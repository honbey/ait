use std::collections::HashSet;
use std::env;
use std::fs;
use std::path::Path;

fn to_pascal_case(s: &str) -> String {
    s.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(c) => {
                    let upper: String = c.to_uppercase().collect();
                    upper + &chars.as_str().to_lowercase()
                }
            }
        })
        .collect()
}

fn main() {
    println!("cargo:rerun-if-changed=locales/zh.json");
    println!("cargo:rerun-if-changed=locales/en.json");

    let zh_path = Path::new("locales/zh.json");
    let zh_content = fs::read_to_string(zh_path).expect("Failed to read locales/zh.json");
    let zh_map: serde_json::Value =
        serde_json::from_str(&zh_content).expect("Failed to parse zh.json");
    let zh_obj = zh_map.as_object().expect("zh.json must be a JSON object");

    let en_path = Path::new("locales/en.json");
    let en_content = fs::read_to_string(en_path).expect("Failed to read locales/en.json");
    let en_map: serde_json::Value =
        serde_json::from_str(&en_content).expect("Failed to parse en.json");
    let en_obj = en_map.as_object().expect("en.json must be a JSON object");

    let zh_keys: HashSet<&str> = zh_obj.keys().map(|k| k.as_str()).collect();
    let en_keys: HashSet<&str> = en_obj.keys().map(|k| k.as_str()).collect();

    for k in zh_keys.difference(&en_keys) {
        println!("cargo:warning=zh.json has key \"{k}\" but en.json does not");
    }
    for k in en_keys.difference(&zh_keys) {
        println!("cargo:warning=en.json has key \"{k}\" but zh.json does not");
    }

    let mut keys: Vec<&str> = zh_keys.into_iter().collect();
    keys.sort();

    let mut code = String::new();

    code.push_str("#[derive(Clone, Copy, Debug, PartialEq, Eq)]\n");
    code.push_str("#[allow(dead_code)]\n");
    code.push_str("pub enum K {\n");
    for key in &keys {
        let variant = to_pascal_case(key);
        code.push_str(&format!("    {variant},\n"));
    }
    code.push_str("}\n\n");

    code.push_str("impl K {\n");
    code.push_str("    pub fn as_str(&self) -> &'static str {\n");
    code.push_str("        match self {\n");
    for key in &keys {
        let variant = to_pascal_case(key);
        code.push_str(&format!("            Self::{variant} => \"{key}\",\n"));
    }
    code.push_str("        }\n");
    code.push_str("    }\n");
    code.push_str("}\n");

    let out_dir = env::var("OUT_DIR").unwrap();
    let dest_path = Path::new(&out_dir).join("i18n_keys.rs");
    fs::write(&dest_path, &code).expect("Failed to write i18n_keys.rs");
}
