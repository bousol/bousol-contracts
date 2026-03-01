#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, token, Address, Env, Error};

#[contract]
pub struct RoscaContract;

#[derive(Clone, Debug, Eq, PartialEq)]
#[contracttype]
pub enum DataKey {
  // Counter,
  // Proposal(u32),
  // Voter(u32, Address),
  Admin,
}

#[contractimpl]
impl RoscaContract {
  pub fn __constructor(env: Env, admin: Address) {
    // Ensure contract isn't already initialized
    if env.storage().instance().has(&DataKey::Admin) {
      panic!("Contract already initialized");
    }
    // Store admin address
    env.storage().instance().set(&DataKey::Admin, &admin);
  }

  // /// Get the current admin
  pub fn get_admin(env: Env) -> Address {
    if !env.storage().instance().has(&DataKey::Admin) {
      panic!("Contract not initialized");
    }
    env.storage().instance().get(&DataKey::Admin).unwrap()
    // Address::from_string(&String::from_str(&env, DEFAULT_ADMIN))
  }

  fn is_admin(env: &Env, caller: &Address) -> bool {
    caller.require_auth();
    match env.storage().instance().get(&DataKey::Admin) {
      Some(stored_admin) => caller == &stored_admin,
      None => panic!("Admin not set")
    }
  }

  pub fn transfer_funds(
    env: Env,
    caller: Address,
    token: Address,
    to: Address,
    amount: i128,
  ) -> Result<(), Error> {
    if !Self::is_admin(&env, &caller) {
      panic!("Caller is not admin");
    }
    // Create token client
    let token_client = token::Client::new(&env, &token);

    // Get contract's own address
    let contract_address = env.current_contract_address();

    // Transfer from contract to recipient
    token_client.transfer(
      &contract_address, // from (contract address)
      &to,               // to (recipient address)
      &amount,           // amount to transfer
    );
    Ok(())
  }
}

mod test;
