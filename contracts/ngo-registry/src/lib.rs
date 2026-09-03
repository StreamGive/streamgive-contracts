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
        Ok(())
    }

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

        env.events()
            .publish((symbol_short!("approved"), ngo_owner), ());

        Ok(())
    }
}

mod test;
