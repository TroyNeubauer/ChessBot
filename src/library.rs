use indexmap::IndexMap;
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::mem::transmute;

type UserUuid = u32;
type BookUuid = u32;
type CheckoutUuid = u32;

type TimeType = chrono::DateTime<chrono::offset::Local>;

//The following types all have uuids that can be passed around as "referencnes" because they
//uniquely identify an object
#[derive(Serialize, Deserialize, Debug)]
pub struct Book {
    pub uuid: BookUuid,
    pub name: String,
    pub author: String,
    pub total_count: u32,
}

//Represents the 4 stages of a handout
//First a user creates a request with !library checkout. No book han been transacted yet so
//PreTransact represents this phase.
//Next after an offecer hands out the book, they will approve the request in discord by adding a
//thumbs up reaction to the bot's log message that corrorsponds to the rentee.
//Adding this reaction confirms that the rentee has recived the book and their rental timer starts.
//This is the Reading phase. Within a set amount of time (usually 7 days) the rentee will return
//the book to an officer and use the !library return command to confirm this from their side. the
//return commad moves this transaction into the ReturnVerifyNeeded phase. Next, an officer will
//react to a corrorsponding message from the bot to sign off that the book was returned.
//At this point the checkout is complete (Done phase) and the book is ready to be checked out
//again.
#[derive(Serialize, Deserialize, Debug)]
pub enum CheckoutStatus {
    PreTransact,
    Reading,
    ReturnVerifyNeeded,
    DONE,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct OfficerApproval {
    user: UserUuid,
    time: TimeType,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct CheckoutInstance {
    pub uuid: CheckoutUuid,
    pub rentee: UserUuid,
    pub book: BookUuid,
    pub status: CheckoutStatus,
    pub due_date: Option<TimeType>,
    pub checkout_approval: Option<OfficerApproval>,
    pub checkin_approval: Option<OfficerApproval>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct User {
    pub discord_id: String,
    pub read_name: String,
    pub uuid: UserUuid,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Database {
    pub books: IndexMap<BookUuid, Book>,
    pub checkouts: IndexMap<CheckoutUuid, CheckoutInstance>,
    pub users: IndexMap<UserUuid, User>,
}

#[derive(Debug)]
pub enum ManipulationError {
    //Uuid of each checkout thas is still active
    OutstandingBooksNonReturned(Vec<CheckoutUuid>),
    UnknownBook,
}

const LIBRARY_DB_NAME: &str = "library-db.bin";

pub enum UuidError {
    InvalidEncoding,
    NotFound,
    MismatchIsUserUuid,
    MismatchIsBookUuid,
    MismatchIsCheckoutUuid,
}

impl Database {
    pub fn new() -> Database {
        Database {
            books: IndexMap::new(),
            checkouts: IndexMap::new(),
            users: IndexMap::new(),
        }
    }

    pub async fn load() -> Option<Database> {
        let task = tokio::fs::read(LIBRARY_DB_NAME).await;
        match task {
            Ok(data) => {
                let result: Result<Database, _> = bincode::deserialize(&data);

                //We want to panic on failure
                let db = result.unwrap();
                println!("Loaded library: {:?} from disk successfully", db);
                Some(db)
            }
            Err(err) => {
                println!("Failed to load library file: {:?}", err);
                None
            }
        }
    }

    pub async fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let data: Vec<u8> = bincode::serialize(self)?;
        tokio::fs::write(LIBRARY_DB_NAME, data).await?;

        println!("Saved library database successfully");
        Ok(())
    }

    pub async fn try_save(&self) {
        match self.save().await {
            Ok(_) => {}
            Err(err) => {
                println!("An error occured while trying to save thi library database!");
                println!("{:?}", err);
                println!("Dumping database json to stdout:");
                let json = serde_json::to_string(&self).unwrap();
                println!("{}", json);

                let mut temp_file = std::env::temp_dir();
                temp_file.push("ERAU-chess-bot-");
                let num: u32 = rand::thread_rng().gen();
                temp_file.push(format!("{:x}", num));
                temp_file.push(".json");

                println!("Also writing to temp file {:?}", temp_file.to_str());
                match tokio::fs::write(temp_file, json).await {
                    Err(err) => println!("Failed to save backup json!: {}", err),
                    Ok(_) => {}
                }
            }
        }
    }

    fn add_book(&mut self, book: Book) -> Result<(), ManipulationError> {
        self.books.insert(book.uuid, book);

        Ok(())
    }

    fn remove_book(&mut self, uuid: BookUuid) -> Result<Book, ManipulationError> {
        for i in 0..self.checkouts.len() {
            if self.checkouts[i].book == uuid {
                //The book we are trying to remove is un accounted for
                //Get the list of

                let mut error_books: Vec<BookUuid> = Vec::new();
                for j in 0..self.checkouts.len() {
                    if self.checkouts[j].book == uuid {
                        error_books.push(self.checkouts[j].uuid);
                    }
                }

                return Err(ManipulationError::OutstandingBooksNonReturned(error_books));
            }
        }

        let opt_book = self.books.remove(&uuid);
        match opt_book {
            None => Err(ManipulationError::UnknownBook),
            Some(book) => Ok(book),
        }
    }

    fn decode_raw(&self, uuid: &str) -> Result<u32, UuidError> {
        let len_needed = match data_encoding::BASE32.decode_len(uuid.len()) {
            Err(_) => return Err(UuidError::InvalidEncoding),
            Ok(len) => len,
        };
        if len_needed > 4 {
            return Err(UuidError::NotFound);
        }
        let mut decoded = [0; 4];
        let _ = data_encoding::BASE32.decode_mut(uuid.as_bytes(), &mut decoded);
        Ok(u32::from_be_bytes(decoded))
    }

    pub fn decode_user_uuid(&self, uuid: &str) -> Result<UserUuid, UuidError> {
        let decoded = self.decode_raw(uuid)?;
        if self.users.contains_key(&decoded) {
            Ok(decoded)
        } else if self.books.contains_key(&decoded) {
            Err(UuidError::MismatchIsBookUuid)
        } else if self.checkouts.contains_key(&decoded) {
            Err(UuidError::MismatchIsCheckoutUuid)
        } else {
            Err(UuidError::NotFound)
        }
    }

    pub fn decode_book_uuid(&self, uuid: &str) -> Result<UserUuid, UuidError> {
        let decoded = self.decode_raw(uuid)?;
        if self.books.contains_key(&decoded) {
            Ok(decoded)
        } else if self.users.contains_key(&decoded) {
            Err(UuidError::MismatchIsUserUuid)
        } else if self.checkouts.contains_key(&decoded) {
            Err(UuidError::MismatchIsCheckoutUuid)
        } else {
            Err(UuidError::NotFound)
        }
    }

    pub fn decode_checkout_uuid(&self, uuid: &str) -> Result<UserUuid, UuidError> {
        let decoded = self.decode_raw(uuid)?;
        if self.checkouts.contains_key(&decoded) {
            Ok(decoded)
        } else if self.books.contains_key(&decoded) {
            Err(UuidError::MismatchIsBookUuid)
        } else if self.users.contains_key(&decoded) {
            Err(UuidError::MismatchIsUserUuid)
        } else {
            Err(UuidError::NotFound)
        }
    }

    pub fn encode_uuid(uuid: u32) -> String {
        let bytes: [u8; 4] = uuid.to_be_bytes();
        data_encoding::BASE32.encode(&bytes[0..4])
    }
}
