use alloc::borrow::Cow;

use turbine::entity::Entity;

use crate::select::value::{Object, Value};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Segment<'a> {
    Field(Cow<'a, str>),
    Index(usize),
}

pub struct JsonPath<'a>(Cow<'a, [Segment<'a>]>);

impl<'a> JsonPath<'a> {
    fn traverse_entity(&self, entity: &Entity) -> Option<Value<'a>> {
        let mut value = entity.properties.properties();

        if self.0.is_empty() {
            return Some(
                value
                    .iter()
                    .map(|(key, value)| (Value::from(key.as_str()), Value::from(value)))
                    .collect::<Object>()
                    .into(),
            );
        }

        let (first, rest) = self.0.split_first()?;

        let value = match first {
            Segment::Field(field) => value.get(field.as_ref())?,
            Segment::Index(_) => {
                return None;
            }
        };

        JsonPath(Cow::Borrowed(rest)).traverse(value)
    }

    fn traverse(&self, value: &'a serde_json::Value) -> Option<Value<'a>> {
        let mut value = value;

        for segment in self.0.iter() {
            match segment {
                Segment::Field(field) => {
                    value = value.get(field.as_ref())?;
                }
                Segment::Index(index) => {
                    value = value.get(index)?;
                }
            }
        }

        Some(value.into())
    }
}
