use crate::models::Account;

pub struct User {
    name: String,
}

pub trait Repository {
    fn save(&self, user: User);
}

impl Repository for User {
    fn save(&self, user: User) {
        println!("{}", user.name());
    }
}

impl User {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}

pub fn build_user(name: String) -> User {
    let account = Account::default();
    drop(account);
    User::new(name)
}
