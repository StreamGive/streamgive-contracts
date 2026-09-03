#![no_std]

use soroban_sdk::{contract, contracterror, contracttype, contractimpl, token, Address, Env};

mod math;

/// A single donor -> NGO streaming donation.
///
/// `balance` is the undrawn amount still deposited in the vault; `rate` is
/// how much of it accrues to the NGO per second. Accrual math lands in a
/// later commit — this is just the storage shape.
#[contracttype]
#[derive(Clone)]
pub struct Stream {
    pub donor: Address,
    pub ngo: Address,
    pub token: Address,
    pub rate: i128,
    pub balance: i128,
    pub withdrawn: i128,
    pub last_update: u64,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    NextStreamId,
    Stream(u64),
    Paused,
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    StreamNotFound = 3,
    InvalidAmount = 4,
    NothingToWithdraw = 5,
    ContractPaused = 6,
}

/// Returns `Err(Error::ContractPaused)` if an admin has paused the vault.
/// Checked at the top of every fund-moving entry point.
fn require_not_paused(env: &Env) -> Result<(), Error> {
    let paused: bool = env
        .storage()
        .instance()
        .get(&DataKey::Paused)
        .unwrap_or(false);
    if paused {
        return Err(Error::ContractPaused);
    }
    Ok(())
}

#[contract]
pub struct DonationVault;

#[contractimpl]
impl DonationVault {
    /// Sets the vault admin and seeds the stream-id counter. Can only be called once.
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextStreamId, &0u64);
        Ok(())
    }

    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Reads back a stream by id.
    pub fn get_stream(env: Env, stream_id: u64) -> Result<Stream, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Stream(stream_id))
            .ok_or(Error::StreamNotFound)
    }

    /// Halts stream creation, withdrawal, top-up, and rate changes.
    /// Admin-gated emergency brake; existing balances stay put and
    /// `cancel_stream` still works so donors can always get a refund.
    pub fn pause(env: Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::Paused, &true);
        Ok(())
    }

    pub fn unpause(env: Env) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::Paused, &false);
        Ok(())
    }

    pub fn paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Opens a new stream: pulls `deposit` of `token` from the donor into the
    /// vault, to be released to the NGO at `rate` per second on withdrawal.
    pub fn create_stream(
        env: Env,
        donor: Address,
        ngo: Address,
        token: Address,
        deposit: i128,
        rate: i128,
    ) -> Result<u64, Error> {
        require_not_paused(&env)?;
        donor.require_auth();

        if deposit <= 0 || rate <= 0 {
            return Err(Error::InvalidAmount);
        }

        let token_client = token::Client::new(&env, &token);
        token_client.transfer(&donor, &env.current_contract_address(), &deposit);

        let stream_id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::NextStreamId)
            .unwrap_or(0);

        let stream = Stream {
            donor,
            ngo,
            token,
            rate,
            balance: deposit,
            withdrawn: 0,
            last_update: env.ledger().timestamp(),
        };

        env.storage()
            .persistent()
            .set(&DataKey::Stream(stream_id), &stream);
        env.storage()
            .instance()
            .set(&DataKey::NextStreamId, &(stream_id + 1));

        Ok(stream_id)
    }

    /// Pays out everything accrued to the NGO since the last checkpoint.
    /// NGO-auth-gated.
    pub fn withdraw(env: Env, stream_id: u64) -> Result<i128, Error> {
        require_not_paused(&env)?;

        let key = DataKey::Stream(stream_id);
        let mut stream: Stream = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::StreamNotFound)?;

        stream.ngo.require_auth();

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stream.last_update);
        let accrued = math::accrued(stream.rate, elapsed, stream.balance);

        if accrued <= 0 {
            return Err(Error::NothingToWithdraw);
        }

        stream.balance -= accrued;
        stream.withdrawn += accrued;
        stream.last_update = now;
        env.storage().persistent().set(&key, &stream);

        let token_client = token::Client::new(&env, &stream.token);
        token_client.transfer(&env.current_contract_address(), &stream.ngo, &accrued);

        Ok(accrued)
    }

    /// Stops a stream for good: settles whatever has already accrued to the
    /// NGO (so cancelling doesn't claw back funds already earned), refunds
    /// the untouched remainder to the donor, then zeroes the stream's rate
    /// and balance. Donor-auth-gated. The record is kept, not deleted, so
    /// the stream's history stays queryable.
    pub fn cancel_stream(env: Env, stream_id: u64) -> Result<(), Error> {
        let key = DataKey::Stream(stream_id);
        let mut stream: Stream = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::StreamNotFound)?;

        stream.donor.require_auth();

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stream.last_update);
        let accrued = math::accrued(stream.rate, elapsed, stream.balance);

        let token_client = token::Client::new(&env, &stream.token);

        if accrued > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.ngo, &accrued);
            stream.withdrawn += accrued;
            stream.balance -= accrued;
        }

        let refund = stream.balance;
        if refund > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.donor, &refund);
        }

        stream.balance = 0;
        stream.rate = 0;
        stream.last_update = now;
        env.storage().persistent().set(&key, &stream);

        Ok(())
    }

    /// Adds more funds to an existing stream. Donor-auth-gated. Settles
    /// whatever has already accrued to the NGO first, so the top-up only
    /// ever affects accrual going forward.
    pub fn top_up(env: Env, stream_id: u64, amount: i128) -> Result<(), Error> {
        require_not_paused(&env)?;

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let key = DataKey::Stream(stream_id);
        let mut stream: Stream = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::StreamNotFound)?;

        stream.donor.require_auth();

        let token_client = token::Client::new(&env, &stream.token);

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stream.last_update);
        let accrued = math::accrued(stream.rate, elapsed, stream.balance);
        if accrued > 0 {
            token_client.transfer(&env.current_contract_address(), &stream.ngo, &accrued);
            stream.balance -= accrued;
            stream.withdrawn += accrued;
        }
        stream.last_update = now;

        token_client.transfer(&stream.donor, &env.current_contract_address(), &amount);
        stream.balance += amount;

        env.storage().persistent().set(&key, &stream);
        Ok(())
    }

    /// Changes the per-second accrual rate on an existing stream. Donor-auth-gated.
    /// Settles whatever has already accrued at the old rate first, so the new
    /// rate only ever applies going forward — never retroactively.
    pub fn modify_rate(env: Env, stream_id: u64, new_rate: i128) -> Result<(), Error> {
        require_not_paused(&env)?;

        if new_rate <= 0 {
            return Err(Error::InvalidAmount);
        }

        let key = DataKey::Stream(stream_id);
        let mut stream: Stream = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::StreamNotFound)?;

        stream.donor.require_auth();

        let now = env.ledger().timestamp();
        let elapsed = now.saturating_sub(stream.last_update);
        let accrued = math::accrued(stream.rate, elapsed, stream.balance);
        if accrued > 0 {
            let token_client = token::Client::new(&env, &stream.token);
            token_client.transfer(&env.current_contract_address(), &stream.ngo, &accrued);
            stream.balance -= accrued;
            stream.withdrawn += accrued;
        }
        stream.last_update = now;
        stream.rate = new_rate;

        env.storage().persistent().set(&key, &stream);
        Ok(())
    }
}

mod test;
