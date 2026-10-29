use core::fmt;
use heapless::{String, Vec};
use serde::{
    de::{self, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use time::macros::format_description;
use time::Date;
use time::Time;

// Custom deserializer for vector of ISO datetimes to heapless::Vec<DateTime, N>
pub fn deserialize_option_date_vec<'de, D, const N: usize>(
    deserializer: D,
) -> Result<Option<Vec<Date, N>>, D::Error>
where
    D: Deserializer<'de>,
{
    struct DateOptionVisitor<const N: usize>;

    impl<'de, const N: usize> Visitor<'de> for DateOptionVisitor<N> {
        type Value = Option<Vec<Date, N>>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("null or a sequence of yyyy-mm-dd date strings")
        }

        fn visit_none<E>(self) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_seq(DateVecVisitor::<N>).map(Some)
        }
    }

    struct DateVecVisitor<const N: usize>;

    impl<'de, const N: usize> Visitor<'de> for DateVecVisitor<N> {
        type Value = Vec<Date, N>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a sequence of yyyy-mm-dd date strings")
        }

        fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let mut vec = Vec::new();

            while let Some(element) = seq.next_element_seed(DateSeed)? {
                vec.push(element)
                    .map_err(|_| de::Error::custom("exceeded maximum capacity"))?;
            }

            Ok(vec)
        }
    }

    // Seed for deserializing individual Date on the format yyyy-mm-dd
    struct DateSeed;

    impl<'de> serde::de::DeserializeSeed<'de> for DateSeed {
        type Value = Date;

        fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
        where
            D: Deserializer<'de>,
        {
            let s = String::<20>::deserialize(deserializer)?;
            let date_format = format_description!("[year]-[month]-[day]");
            Date::parse(&s, &date_format)
                .map_err(|_e| de::Error::custom("Failed to deserialize date"))
        }
    }

    deserializer.deserialize_option(DateOptionVisitor::<N>)
}

// Custom deserializer for vector of ISO datetimes to heapless::Vec<DateTime, N>
pub fn deserialize_time<'de, D>(deserializer: D) -> Result<Time, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::<10>::deserialize(deserializer)?;
    let time_format = format_description!("[hour]:[minute]:[second]");
    Time::parse(&s, &time_format).map_err(|_e| de::Error::custom("Failed to deserialize time"))
}
