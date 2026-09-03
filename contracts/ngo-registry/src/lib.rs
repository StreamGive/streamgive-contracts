#![no_std]

use soroban_sdk::{contract, contracterror, contracttype, contractimpl, Address, Env, String};

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
}
