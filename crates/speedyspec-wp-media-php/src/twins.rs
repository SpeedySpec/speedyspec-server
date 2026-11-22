//! The PHP twin interface of the media reader port, which a PHP object implements to replace the reader.

use speedyspec_php_ext::{PortError, Value, php_port};

/// `read(string $path): array|false`, returning the neutral array (`schema` 1) or `false`.
#[php_port(name = "SpeedySpec\\WpMedia\\MediaReader")]
pub trait MediaReaderTwin {
    fn read(&self, path: Value) -> Result<Value, PortError>;
}
