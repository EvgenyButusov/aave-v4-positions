/// What a label read, a write or a listing read refuses with.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the token label read failed")]
    LabelsReadFailed(#[source] postgres::Error),

    #[error("the token metadata write failed")]
    WriteFailed(#[source] postgres::Error),

    #[error("the Hub's token listings could not be read")]
    ListingsReadFailed(#[source] Box<dyn std::error::Error + Send + Sync>),

    /// A column held something its type says it cannot.
    #[error("{column} is not a {expected}: {value}")]
    Malformed {
        column: &'static str,
        expected: &'static str,
        value: String,
    },
}
