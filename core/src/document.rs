//! Lossless JSON document validation and calculation projection. Never execute prose.
use serde::{
    de::{self, MapAccess, SeqAccess, Visitor},
    Deserialize, Deserializer,
};
use serde_json::{Map, Value};
use std::fmt;
pub struct StrictValue(pub Value);
impl<'de> Deserialize<'de> for StrictValue {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = StrictValue;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("JSON without duplicate keys")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut m = Map::new();
                while let Some(k) = a.next_key::<String>()? {
                    if m.contains_key(&k) {
                        return Err(de::Error::custom("duplicate JSON key"));
                    }
                    m.insert(k, a.next_value::<StrictValue>()?.0);
                }
                Ok(StrictValue(Value::Object(m)))
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                let mut v = vec![];
                while let Some(x) = a.next_element::<StrictValue>()? {
                    v.push(x.0);
                }
                Ok(StrictValue(Value::Array(v)))
            }
            fn visit_str<E: de::Error>(self, v: &str) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v.into())))
            }
            fn visit_string<E: de::Error>(self, v: String) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::String(v)))
            }
            fn visit_bool<E: de::Error>(self, v: bool) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Bool(v)))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<Self::Value, E> {
                Ok(StrictValue(v.into()))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(v)
                    .map(|n| StrictValue(Value::Number(n)))
                    .ok_or_else(|| de::Error::custom("nonfinite number"))
            }
            fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
                Ok(StrictValue(Value::Null))
            }
            fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
                self.visit_unit()
            }
        }
        d.deserialize_any(V)
    }
}
pub fn without_comments(v: &Value) -> Result<Value, String> {
    Ok(match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, v) in m {
                if k == "comment" {
                    if !v.is_string() {
                        return Err("structure: comment must be a string".into());
                    }
                } else {
                    out.insert(k.clone(), without_comments(v)?);
                }
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(without_comments).collect::<Result<_, _>>()?),
        _ => v.clone(),
    })
}
pub fn calculations(v: &Value) -> Result<Value, String> {
    let c: crate::model::Strategy =
        serde_json::from_value(v.clone()).map_err(|_| "structure: invalid strategy")?;
    crate::validate(&c)?;
    let mut out = serde_json::to_value(c).map_err(|_| "strategy serialization")?;
    out.as_object_mut().unwrap().remove("messages");
    // Rule ordering is not a policy: identities select algorithms.
    for key in ["weekly_rules", "daily_rules"] {
        out["policies"]["plans"][key]
            .as_array_mut()
            .unwrap()
            .sort_by_key(Value::to_string);
    }
    Ok(out)
}
