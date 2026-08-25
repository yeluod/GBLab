use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};
use thiserror::Error;

const GB_DEVICE_ID_LENGTH: usize = 20;

/// 经过格式校验的 GB28181 设备国标编号。
#[derive(Clone, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(transparent)]
pub struct DeviceId(String);

impl DeviceId {
    /// 校验并创建设备编号。
    ///
    /// # Errors
    ///
    /// 编号不是 20 位 ASCII 数字时返回 [`DeviceIdError`]。
    pub fn new(value: impl Into<String>) -> Result<Self, DeviceIdError> {
        let value = value.into();
        if value.len() != GB_DEVICE_ID_LENGTH {
            return Err(DeviceIdError::InvalidLength {
                actual: value.len(),
            });
        }
        if !value.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(DeviceIdError::NonDigit);
        }
        Ok(Self(value))
    }

    /// 返回国标编号字符串。
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for DeviceId {
    type Err = DeviceIdError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::new(value)
    }
}

/// 设备国标编号格式错误。
#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum DeviceIdError {
    /// 编号长度不是 20 字节。
    #[error("设备国标编号必须为 20 位，实际为 {actual} 位")]
    InvalidLength {
        /// 输入的实际字节长度。
        actual: usize,
    },
    /// 编号包含非 ASCII 数字字符。
    #[error("设备国标编号只能包含 ASCII 数字")]
    NonDigit,
}

#[cfg(test)]
mod tests {
    use super::{DeviceId, DeviceIdError};

    #[test]
    fn new_should_preserve_value_when_device_id_is_valid() -> Result<(), DeviceIdError> {
        let device_id = DeviceId::new("34020000001320000001")?;

        assert_eq!(device_id.as_str(), "34020000001320000001");
        Ok(())
    }

    #[test]
    fn new_should_return_length_error_when_device_id_is_too_short() {
        let result = DeviceId::new("3402000000132000000");

        assert_eq!(result, Err(DeviceIdError::InvalidLength { actual: 19 }));
    }

    #[test]
    fn new_should_return_non_digit_error_when_device_id_contains_letter() {
        let result = DeviceId::new("3402000000132000000A");

        assert_eq!(result, Err(DeviceIdError::NonDigit));
    }
}
