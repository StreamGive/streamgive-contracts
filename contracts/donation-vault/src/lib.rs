#![no_std]

use soroban_sdk::{contract, contracterror, contracttype, contractimpl, token, Address, Env};

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
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    StreamNotFound = 3,
    InvalidAmount = 4,
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
}

mod test;
