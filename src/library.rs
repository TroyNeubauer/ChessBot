use indexmap::IndexMap;
use rand::Rng;
use serde::{Deserialize, Serialize};

#[path = "utils.rs"]
mod utils;

pub type UserUuid = u32;
pub type BookUuid = u32;
pub type CheckoutUuid = u32;

pub type TimeType = chrono::DateTime<chrono::offset::Local>;

//The following types all have uuids that can be passed around as "referencnes" because they
//uniquely identify an object
#[derive(Serialize, Deserialize, Debug, new)]
pub struct Book {
    pub uuid: BookUuid,
    pub name: String,
    pub author: String,
    pub quantity: u32,
}

//Represents the 4 stages of a handout
//First a user creates a request with !library checkout. No book han been transacted yet so
//PreTransact represents this phase.
//Next after an officer hands out the book, they will approve the request in discord by adding a
//thumbs up reaction to the bot's log message that corrorsponds to the rentee.
//Adding this reaction confirms that the rentee has recieved the book and their rental timer starts.
//This is the Reading phase. Within a set amount of time (usually 7 days) the rentee will return
//the book to an officer and use the !library return command to confirm this from their side. the
//return command moves this transaction into the ReturnVerifyNeeded phase. Next, an officer will
//react to a corrorsponding message from the bot to sign off that the book was returned.
//At this point the checkout is complete (Done phase) and the book is ready to be checked out
//again.
#[derive(Serialize, Deserialize, Debug, new)]
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

#[derive(Serialize, Deserialize, Debug, new)]
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

#[derive(Debug, new)]
pub struct ManipulationError(ManipulationErrorType);

impl std::error::Error for ManipulationError {}

impl std::fmt::Display for ManipulationError {
    fn fmt(&self, fmt: &mut std::fmt::Formatter<'_>) -> std::result::Result<(), std::fmt::Error> {
        match &self.0 {
            ManipulationErrorType::AlreadyAdded(input) => write!(
                fmt,
                "Book \"{}\" already in library. Use !library set-quantity <book> <new quantity> to indicate that the library has 2 or more copies of a book",
                input
            ),
            ManipulationErrorType::OutstandingBooksNonReturned(vec) => {
                write!(fmt, "Book already checked out! Checkout ids:  ")?;
                for checkout in vec {
                    write!(fmt, "ID: {}, ", Database::encode_uuid(checkout.clone()))?;
                }
                write!(fmt, "\nUse !library list to see more checkout information")
            },
            ManipulationErrorType::UnknownBook(input) => write!(fmt, "Unknown book: \"{}\"", input),
        }
    }
}

#[derive(Debug)]
pub enum ManipulationErrorType {
    //Uuid of each checkout thats is still active
    OutstandingBooksNonReturned(Vec<CheckoutUuid>),
    UnknownBook(String),
    AlreadyAdded(String),
}

const LIBRARY_DB_NAME: &str = "library-db.bin";

#[derive(Debug)]
pub enum UuidError {
    InvalidEncoding,
    NotFound,
    MismatchIsUserUuid,
    MismatchIsBookUuid,
    MismatchIsCheckoutUuid,
}

#[derive(Debug, PartialEq, Eq)]
pub enum UuidType {
    User,
    Book,
    Checkout,
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
                let num: u32 = rand::thread_rng().gen();
                temp_file.push(format!("Chess-bot-DB-dump-{:x}.json", num));

                println!("Also writing to temp file {:?}", temp_file.to_str());
                match tokio::fs::write(temp_file, json).await {
                    Err(err) => println!("Failed to save backup json!: {}", err),
                    Ok(_) => {}
                }
            }
        }
    }

    pub fn add_book(&mut self, book: Book) -> Result<(), ManipulationError> {
        if self.books.contains_key(&book.uuid) {
            return Err(ManipulationError::new(ManipulationErrorType::AlreadyAdded(
                Database::encode_uuid(book.uuid),
            )));
        }
        for existing_book in &self.books {
            if utils::cmp_ignore_case_ascii(&existing_book.1.name, &book.name)
                && utils::cmp_ignore_case_ascii(&existing_book.1.author, &book.author)
            {
                return Err(ManipulationError::new(ManipulationErrorType::AlreadyAdded(
                    book.name,
                )));
            }
        }
        self.books.insert(book.uuid, book);

        Ok(())
    }

    pub fn remove_book(&mut self, uuid: BookUuid) -> Result<Book, ManipulationError> {
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

                return Err(ManipulationError::new(
                    ManipulationErrorType::OutstandingBooksNonReturned(error_books),
                ));
            }
        }

        let opt_book = self.books.remove(&uuid);
        match opt_book {
            None => Err(ManipulationError::new(ManipulationErrorType::UnknownBook(
                Database::encode_uuid(uuid),
            ))),
            Some(book) => Ok(book),
        }
    }

    fn new_raw_uuid(&self) -> u32 {
        loop {
            let mut rng = rand::thread_rng();
            let uuid: u32 = rng.gen();

            if uuid < u32::pow(2, 32 - 5) {
                //Make sure that numbers that would require base32 padding because the high bits
                //are not set are not generated
                continue;
            }

            if !self.users.contains_key(&uuid)
                && !self.books.contains_key(&uuid)
                && !self.checkouts.contains_key(&uuid)
            {
                return uuid;
            }
        }
    }

    pub fn new_book_uuid(&self) -> BookUuid {
        self.new_raw_uuid()
    }

    pub fn new_user_uuid(&self) -> UserUuid {
        self.new_raw_uuid()
    }

    pub fn new_checkout_uuid(&self) -> CheckoutUuid {
        self.new_raw_uuid()
    }

    fn decode_raw(&self, uuid: &str) -> Result<(u32, UuidType), UuidError> {
        let len_needed = match data_encoding::BASE32_NOPAD.decode_len(uuid.len()) {
            Err(_) => return Err(UuidError::InvalidEncoding),
            Ok(len) => len,
        };
        let mut decoded = [0; 4];
        if decoded.len() != len_needed {
            return Err(UuidError::InvalidEncoding);
        }
        let decode_result = data_encoding::BASE32_NOPAD.decode_mut(uuid.as_bytes(), &mut decoded);
        if let Err(_partial) = decode_result {
            return Err(UuidError::InvalidEncoding);
        }
        let result = u32::from_be_bytes(decoded);
        let uuid_type = {
            if self.users.contains_key(&result) {
                UuidType::User
            } else if self.books.contains_key(&result) {
                UuidType::Book
            } else if self.checkouts.contains_key(&result) {
                UuidType::Checkout
            } else {
                return Err(UuidError::NotFound);
            }
        };
        Ok((result, uuid_type))
    }

    fn uuid_type_to_mismatch_error(uuid_type: UuidType) -> UuidError {
        match (uuid_type) {
            UuidType::User => UuidError::MismatchIsUserUuid,
            UuidType::Book => UuidError::MismatchIsBookUuid,
            UuidType::Checkout => UuidError::MismatchIsCheckoutUuid,
            _ => unreachable!(),
        }
    }

    pub fn decode_user_uuid(&self, uuid: &str) -> Result<UserUuid, UuidError> {
        let (decoded, uuid_type) = self.decode_raw(uuid)?;
        if uuid_type != UuidType::User {
            Err(Database::uuid_type_to_mismatch_error(uuid_type))
        } else {
            Ok(decoded)
        }
    }

    pub fn decode_book_uuid(&self, uuid: &str) -> Result<BookUuid, UuidError> {
        let (decoded, uuid_type) = self.decode_raw(uuid)?;
        if uuid_type != UuidType::Book {
            Err(Database::uuid_type_to_mismatch_error(uuid_type))
        } else {
            Ok(decoded)
        }
    }

    pub fn decode_checkout_uuid(&self, uuid: &str) -> Result<CheckoutUuid, UuidError> {
        let (decoded, uuid_type) = self.decode_raw(uuid)?;
        if uuid_type != UuidType::Checkout {
            Err(Database::uuid_type_to_mismatch_error(uuid_type))
        } else {
            Ok(decoded)
        }
    }

    pub fn encode_uuid(uuid: u32) -> String {
        let bytes: [u8; 4] = uuid.to_be_bytes();
        data_encoding::BASE32_NOPAD.encode(&bytes[0..4])
    }

    pub fn get_book_from_input(&self, input: &String) -> Option<&Book> {
        let mut book_opt_uuid = None;

        match self.decode_book_uuid(input) {
            Ok(uuid) =>
            //If the input book is a valid uuid then use that
            {
                book_opt_uuid = Some(uuid)
            }
            Err(err) => {
                println!("failed to parse uuid: \"{}\" - {:?}", input, err);
                for (uuid, book) in &self.books {
                    if utils::cmp_ignore_case_ascii(&book.name, input) {
                        //They inputted the book's name, return the uuid
                        book_opt_uuid = Some(book.uuid);
                        break;
                    }
                }
            } //Could not find book by uuid or name
        }

        match book_opt_uuid {
            Some(book_uuid) => match self.books.get(&book_uuid) {
                Some(book) => Some(book),
                None => unreachable!(),
            },
            None => None,
        }
    }

    pub fn get_book_from_input_mut(&mut self, input: &String) -> Option<&mut Book> {
        let mut book_opt_uuid = None;

        match self.decode_book_uuid(input) {
            Ok(uuid) =>
            //If the input book is a valid uuid then use that
            {
                book_opt_uuid = Some(uuid)
            }
            Err(err) => {
                println!("failed to parse uuid: \"{}\" - {:?}", input, err);
                for (uuid, book) in &self.books {
                    if utils::cmp_ignore_case_ascii(&book.name, input) {
                        //They inputted the book's name, return the uuid
                        book_opt_uuid = Some(book.uuid);
                        break;
                    }
                }
            } //Could not find book by uuid or name
        }

        match book_opt_uuid {
            Some(book_uuid) => match self.books.get_mut(&book_uuid) {
                Some(book) => Some(book),
                None => unreachable!(),
            },
            None => None,
        }
    }
}
