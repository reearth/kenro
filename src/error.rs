/// Errors produced by kenro's spatial functions.
///
/// Every message is prefixed with `kenro: ` so failures are attributable when
/// they surface through SQLite error strings in host-application logs.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum Error {
    #[error("kenro: invalid GeoPackage blob: {0}")]
    InvalidGpb(&'static str),

    #[error("kenro: invalid WKB: {0}")]
    InvalidWkb(String),

    #[error("kenro: invalid WKT: {0}")]
    InvalidWkt(String),

    #[error("kenro: {func}: mixed SRIDs {a} and {b}; reproject with ST_Transform first")]
    MixedSrid { func: &'static str, a: i32, b: i32 },

    #[error("kenro: invalid GeoJSON: {0}")]
    InvalidGeoJson(String),

    #[error("kenro: {func}: {reason}")]
    Unsupported { func: &'static str, reason: String },

    #[error("kenro: {func} is not implemented in kenro. {hint}")]
    Unimplemented {
        func: &'static str,
        hint: &'static str,
    },

    #[error("kenro: {0}")]
    Geozero(#[from] geozero::error::GeozeroError),
}

pub type Result<T> = std::result::Result<T, Error>;
