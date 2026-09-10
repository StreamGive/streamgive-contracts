#![no_std]

use soroban_sdk::{
    contract, contracterror, contracttype, contractimpl, symbol_short, Address, Env, String,
};

#[contracttype]
#[derive(Clone)]
pub struct Ngo {
    pub owner: Address,
    pub name: String,
    pub verified: bool,
}

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Admin,
    Ngo(Address),
}

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    AlreadyInitialized = 1,
    NotInitialized = 2,
    AlreadyRegistered = 3,
    NotRegistered = 4,
}

/// Approximate ledgers per day at a 5-second close time. Used to express
/// storage TTLs (which the network counts in ledgers, not wall time) in
/// human terms.
const DAY_IN_LEDGERS: u32 = 17_280;

const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

const NGO_BUMP_AMOUNT: u32 = 90 * DAY_IN_LEDGERS;
const NGO_LIFETIME_THRESHOLD: u32 = NGO_BUMP_AMOUNT - DAY_IN_LEDGERS;

fn extend_instance_ttl(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

fn extend_ngo_ttl(env: &Env, owner: &Address) {
    env.storage().persistent().extend_ttl(
        &DataKey::Ngo(owner.clone()),
        NGO_LIFETIME_THRESHOLD,
        NGO_BUMP_AMOUNT,
    );
}

#[contract]
pub struct NgoRegistry;

#[contractimpl]
impl NgoRegistry {
    /// Sets the registry admin. Can only be called once.
    pub fn init(env: Env, admin: Address) -> Result<(), Error> {
        if env.storage().instance().has(&DataKey::Admin) {
            return Err(Error::AlreadyInitialized);
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        extend_instance_ttl(&env);
        Ok(())
    }

    /// Reads back the registry admin set by `init`.
    pub fn admin(env: Env) -> Result<Address, Error> {
        env.storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)
    }

    /// Submits an NGO application. Callable by the NGO's own address.
    /// The entry starts unverified until an admin approves it.
    pub fn register(env: Env, owner: Address, name: String) -> Result<(), Error> {
        owner.require_auth();

        let key = DataKey::Ngo(owner.clone());
        if env.storage().persistent().has(&key) {
            return Err(Error::AlreadyRegistered);
        }

        let ngo = Ngo {
            owner: owner.clone(),
            name: name.clone(),
            verified: false,
        };
        env.storage().persistent().set(&key, &ngo);
        extend_instance_ttl(&env);
        extend_ngo_ttl(&env, &owner);

        env.events()
            .publish((symbol_short!("register"), owner), name);

        Ok(())
    }

    /// Reads back an NGO's registry entry, registered or not.
    pub fn get_ngo(env: Env, owner: Address) -> Result<Ngo, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Ngo(owner))
            .ok_or(Error::NotRegistered)
    }

    /// Marks a registered NGO as verified. Admin-only.
    pub fn approve_ngo(env: Env, ngo_owner: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::Ngo(ngo_owner);
        let mut ngo: Ngo = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotRegistered)?;
        ngo.verified = true;
        env.storage().persistent().set(&key, &ngo);
        extend_instance_ttl(&env);
        extend_ngo_ttl(&env, &ngo_owner);

        env.events()
            .publish((symbol_short!("approved"), ngo_owner), ());

        Ok(())
    }

    /// Reverses a prior approval, marking a registered NGO as unverified
    /// again. Admin-only. Returns `Error::NotRegistered` for an address
    /// with no entry, matching `approve_ngo`'s existing behavior.
    pub fn revoke_ngo(env: Env, ngo_owner: Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        admin.require_auth();

        let key = DataKey::Ngo(ngo_owner);
        let mut ngo: Ngo = env
            .storage()
            .persistent()
            .get(&key)
            .ok_or(Error::NotRegistered)?;
        ngo.verified = false;
        env.storage().persistent().set(&key, &ngo);
        extend_instance_ttl(&env);
        extend_ngo_ttl(&env, &ngo_owner);

        env.events()
            .publish((symbol_short!("revoked"), ngo_owner), ());

        Ok(())
    }
}

mod test;
