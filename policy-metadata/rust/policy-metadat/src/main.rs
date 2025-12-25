use serde_json::{json, Value, Map};

pub struct PolicyMetaData {
    data: Value,
}

impl PolicyMetaData {
    // Initialize with a "type" field
    pub fn new(policy_type: &str) -> Self {
        let mut map = Map::new();
        map.insert("type".to_string(), json!(policy_type));
        
        Self {
            data: Value::Object(map),
        }
    }

    // Add a field to the JSON object
    pub fn add_field(&mut self, name: &str, data: &str) {
        if let Some(obj) = self.data.as_object_mut() {
            obj.insert(name.to_string(), json!(data));
        }
    }

    // Serialize the JSON to a string
    pub fn serialize_to_string(&self) -> String {
        self.data.to_string()
    }
}

fn main() {
    let mut md = PolicyMetaData::new("example_policy");
    md.add_field("description", "This is an example policy.");
    md.add_field("version", "1.0");
    println!("Json String: {}", md.serialize_to_string());
}
