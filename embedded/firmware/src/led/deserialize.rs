use heapless::String;
use serde::{de::Error, Deserialize, Deserializer};
use smart_leds::RGB8;

pub fn deserialize_hex<'de, D>(deserializer: D) -> Result<RGB8, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::<8>::deserialize(deserializer)?;
    if !s.starts_with('#') {
        return Err(D::Error::custom("HEX code must start with #"));
    }

    let rgb_codes = s
        .get(1..)
        .ok_or(D::Error::custom("Invalid HEX code length"))?;

    if rgb_codes.len() != 6 {
        return Err(D::Error::custom("HEX code must be exactly 6 hex digits"));
    }

    // Parse the RGB components from hex
    let r = u8::from_str_radix(&rgb_codes[0..2], 16)
        .map_err(|_| D::Error::custom("Invalid red component"))?;
    let g = u8::from_str_radix(&rgb_codes[2..4], 16)
        .map_err(|_| D::Error::custom("Invalid green component"))?;
    let b = u8::from_str_radix(&rgb_codes[4..6], 16)
        .map_err(|_| D::Error::custom("Invalid blue component"))?;

    Ok(RGB8::new(r, g, b))
}
