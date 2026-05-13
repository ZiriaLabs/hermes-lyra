//! JSON-LD edge layer.  The ONLY place where JSON-LD enters the trust
//! path.  Parser and serializer are hand-rolled (no new dependencies)
//! with a fixed @context embedded at compile time.
//!
//! The typed Rust `SkillDescriptor` is the source of truth; JSON-LD is
//! the interchange format.  Every `SkillDescriptor` must round-trip:
//! deserialize(serialize(d)) == d.

use crate::descriptor::{
    DescriptorBuildError, EffectKind, NamedField, Shape, SkillDescriptor,
};

// -- @context (fixed, never fetched from network) --

const CONTEXT_KEY: &str = "@context";

// -- Minimal value enum for our parser ---------------------------

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Null,
    Bool(bool),
    Num(u64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

// -- Serializer --

pub fn to_jsonld(desc: &SkillDescriptor) -> String {
    let mut obj = Vec::new();
    // @context
    let mut ctx = Vec::new();
    ctx.push(("type".into(), Value::Str("lyra:SkillDescriptor".into())));
    obj.push((CONTEXT_KEY.into(), Value::Obj(ctx)));
    obj.push(("type".into(), Value::Str("lyra:SkillDescriptor".into())));
    obj.push(("name".into(), Value::Str(desc.name().into())));
    obj.push(("version".into(), Value::Str(desc.version().into())));
    obj.push(("content_hash".into(), Value::Str(desc.content_hash_hex())));
    obj.push(("input_shape".into(), value_for_shape(desc.input_shape())));
    obj.push(("output_shape".into(), value_for_shape(desc.output_shape())));
    obj.push((
        "effects".into(),
        Value::Arr(desc.effects().iter().map(|e| Value::Str(effect_str(*e).into())).collect()),
    ));
    obj.push((
        "references".into(),
        Value::Arr(desc.references().iter().map(|r| Value::Str(r.clone())).collect()),
    ));
    render_obj(&obj)
}

fn value_for_shape(s: &Shape) -> Value {
    match s {
        Shape::U8 { max_bytes } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("u8".into())));
            o.push(("max_bytes".into(), Value::Num(*max_bytes)));
            Value::Obj(o)
        }
        Shape::U16 { max_bytes } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("u16".into())));
            o.push(("max_bytes".into(), Value::Num(*max_bytes)));
            Value::Obj(o)
        }
        Shape::U32 { max_bytes } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("u32".into())));
            o.push(("max_bytes".into(), Value::Num(*max_bytes)));
            Value::Obj(o)
        }
        Shape::U64 { max_bytes } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("u64".into())));
            o.push(("max_bytes".into(), Value::Num(*max_bytes)));
            Value::Obj(o)
        }
        Shape::String { max_bytes } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("string".into())));
            o.push(("max_bytes".into(), Value::Num(*max_bytes)));
            Value::Obj(o)
        }
        Shape::Bytes { max_bytes } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("bytes".into())));
            o.push(("max_bytes".into(), Value::Num(*max_bytes)));
            Value::Obj(o)
        }
        Shape::Structured { fields } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("structured".into())));
            o.push((
                "fields".into(),
                Value::Arr(
                    fields
                        .iter()
                        .map(|f| {
                            let mut fo = Vec::new();
                            fo.push(("name".into(), Value::Str(f.name.clone())));
                            fo.push(("shape".into(), value_for_shape(&f.shape)));
                            Value::Obj(fo)
                        })
                        .collect(),
                ),
            ));
            Value::Obj(o)
        }
        Shape::List { item, max_items } => {
            let mut o = Vec::new();
            o.push(("type".into(), Value::Str("list".into())));
            o.push(("item".into(), value_for_shape(item)));
            o.push(("max_items".into(), Value::Num(*max_items)));
            Value::Obj(o)
        }
    }
}

fn effect_str(e: EffectKind) -> &'static str {
    match e {
        EffectKind::None => "none",
        EffectKind::FileRead => "file_read",
        EffectKind::FileWrite => "file_write",
        EffectKind::WebRead => "web_read",
        EffectKind::WebWrite => "web_write",
        EffectKind::Terminal => "terminal",
        EffectKind::Llm => "llm",
    }
}

fn render_value(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => if *b { "true" } else { "false" }.into(),
        Value::Num(n) => format!("{}", n),
        Value::Str(s) => format!("\"{}\"", s),
        Value::Arr(arr) => {
            let parts: Vec<_> = arr.iter().map(render_value).collect();
            format!("[{}]", parts.join(", "))
        }
        Value::Obj(obj) => render_obj(obj),
    }
}

fn render_obj(obj: &[(String, Value)]) -> String {
    let parts: Vec<_> = obj
        .iter()
        .map(|(k, v)| format!("\"{}\": {}", k, render_value(v)))
        .collect();
    format!("{{{}}}", parts.join(", "))
}

// -- Parser --
// Hand-rolled recursive descent, returning Result<_, String>.
// No borrow-checker gymnastics — each parse function takes &str and
// returns (Value, remaining &str).

fn peek(s: &str) -> Option<u8> {
    s.as_bytes().first().copied()
}

fn skip_ws(s: &str) -> &str {
    s.trim_start_matches(|c: char| matches!(c, ' ' | '\t' | '\n' | '\r'))
}

fn parse_value(s: &str) -> Result<(Value, &str), String> {
    let s = skip_ws(s);
    match peek(s) {
        None => Err("empty input".into()),
        Some(b'{') => parse_obj(s),
        Some(b'[') => parse_arr(s),
        Some(b'"') => parse_str(s),
        Some(b't') => {
            let rest = ok("true", s).ok_or("expected true")?;
            Ok((Value::Bool(true), rest))
        }
        Some(b'f') => {
            let rest = ok("false", s).ok_or("expected false")?;
            Ok((Value::Bool(false), rest))
        }
        Some(b'n') => {
            let rest = ok("null", s).ok_or("expected null")?;
            Ok((Value::Null, rest))
        }
        Some(b'0'..=b'9') | Some(b'-') => parse_num(s),
        Some(c) => Err(format!("unexpected char: {}", c)),
    }
}

fn ok<'a>(prefix: &str, s: &'a str) -> Option<&'a str> {
    if s.starts_with(prefix) {
        Some(&s[prefix.len()..])
    } else {
        None
    }
}

fn parse_num(s: &str) -> Result<(Value, &str), String> {
    let bytes = s.as_bytes();
    let mut end = 0;
    for i in 0..bytes.len() {
        if matches!(bytes[i], b'0'..=b'9') {
            end = i + 1;
        } else {
            break;
        }
    }
    if end == 0 {
        return Err("expected number".into());
    }
    let num_str = std::str::from_utf8(&bytes[..end]).map_err(|e| e.to_string())?;
    let n: u64 = num_str.parse::<u64>().map_err(|e| e.to_string())?;
    Ok((Value::Num(n), &s[end..]))
}

fn parse_str(s: &str) -> Result<(Value, &str), String> {
    let s = ok("\"", s).ok_or("expected \"")?;
    let bytes = s.as_bytes();
    let mut out = String::new();
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => return Ok((Value::Str(out), &s[i + 1..])),
            b'\\' => {
                i += 1;
                if i >= bytes.len() { return Err("trailing backslash".into()); }
                match bytes[i] {
                    b'"'  => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/'  => out.push('/'),
                    b'n'  => out.push('\n'),
                    b'r'  => out.push('\r'),
                    b't'  => out.push('\t'),
                    b'b'  => out.push('\u{0008}'),
                    b'f'  => out.push('\u{000C}'),
                    // F4: reject unknown escape sequences (e.g. \q, \x, \$).
                    // Lenient acceptance silently mutated user input.
                    other => return Err(format!(
                        "unknown JSON escape sequence \\{}",
                        other as char
                    )),
                }
            }
            b => {
                if b >= 0x80 {
                    let rest = std::str::from_utf8(&bytes[i..])
                        .map_err(|e| format!("bad utf8: {e}"))?;
                    let ch = rest.chars().next().ok_or("utf8 boundary")?;
                    out.push(ch);
                    i += ch.len_utf8();
                } else {
                    out.push(b as char);
                }
            }
        }
        i += 1;
    }
    Err("unterminated string".into())
}

fn parse_arr(s: &str) -> Result<(Value, &str), String> {
    let s = skip_ws(ok("[", s).ok_or("expected [")?);
    if s.starts_with(']') {
        return Ok((Value::Arr(vec![]), &s[1..]));
    }
    let mut items = Vec::new();
    let mut rest = s;
    loop {
        let (v, next) = parse_value(rest)?;
        items.push(v);
        rest = skip_ws(next);
        if let Some(rest_no_comma) = ok("]", rest) {
            return Ok((Value::Arr(items), rest_no_comma));
        }
        rest = skip_ws(ok(",", rest).ok_or("expected , or ]")?);
    }
}

fn parse_obj(s: &str) -> Result<(Value, &str), String> {
    let s = skip_ws(ok("{", s).ok_or("expected {")?);
    if s.starts_with('}') {
        return Ok((Value::Obj(vec![]), &s[1..]));
    }
    let mut pairs = Vec::new();
    let mut rest = s;
    loop {
        let (key, s1) = parse_string_value(rest)?;
        let s1 = skip_ws(ok(":", s1).ok_or("expected :")?);
        let (val, s2) = parse_value(s1)?;
        pairs.push((key, val));
        rest = skip_ws(s2);
        if let Some(rest_no_brace) = ok("}", rest) {
            return Ok((Value::Obj(pairs), rest_no_brace));
        }
        rest = skip_ws(ok(",", rest).ok_or("expected , or }")?);
    }
}

fn parse_string_value(s: &str) -> Result<(String, &str), String> {
    let (v, rest) = parse_str(s)?;
    match v {
        Value::Str(s) => Ok((s, rest)),
        _ => Err("expected string key".into()),
    }
}

fn get_val<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    obj.iter().find(|(k, _)| k.as_str() == key).map(|(_, v)| v)
}

fn get_str<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a str> {
    match get_val(obj, key) {
        Some(Value::Str(s)) => Some(s.as_str()),
        Some(Value::Obj(o)) if o.iter().any(|(k,v)| {
            if k == "@id" { matches!(v, Value::Str(_)) } else { false }
        }) => {
            let id = o.iter().find(|(k, _)| k.as_str() == "@id")
                        .map(|(_,v)| v);
            if let Some(Value::Str(id)) = id { Some(id.as_str()) } else { None }
        }
        _ => None,
    }
}

fn get_num(obj: &[(String, Value)], key: &str) -> Option<u64> {
    match get_val(obj, key) {
        Some(Value::Num(n)) => Some(*n),
        _ => None,
    }
}

fn get_arr<'a>(obj: &'a [(String, Value)], key: &str) -> Option<&'a [Value]> {
    match get_val(obj, key) {
        Some(Value::Arr(a)) => Some(a.as_slice()),
        _ => None,
    }
}

fn parse_shape_value(v: &Value) -> Result<Shape, String> {
    match v {
        Value::Obj(obj) => parse_shape_obj(obj),
        _ => Err("expected shape object".into()),
    }
}

fn parse_shape_obj(obj: &[(String, Value)]) -> Result<Shape, String> {
    let shape_type = get_str(obj, "type").or_else(|| get_str(obj, "@type")).ok_or("shape missing type")?;
    // F1: `max_bytes` is *required* for every leaf shape. Silent clamping
    // (`.unwrap_or(default).min(width)`) mutated user inputs without
    // surfacing the discrepancy — fail loudly instead.
    let need_max_bytes = || -> Result<u64, String> {
        get_num(obj, "max_bytes")
            .ok_or_else(|| format!("{shape_type:?} shape missing required `max_bytes`"))
    };
    match shape_type {
        "u8"     => Ok(Shape::U8     { max_bytes: need_max_bytes()? }),
        "u16"    => Ok(Shape::U16    { max_bytes: need_max_bytes()? }),
        "u32"    => Ok(Shape::U32    { max_bytes: need_max_bytes()? }),
        "u64"    => Ok(Shape::U64    { max_bytes: need_max_bytes()? }),
        "string" => Ok(Shape::String { max_bytes: need_max_bytes()? }),
        "bytes"  => Ok(Shape::Bytes  { max_bytes: need_max_bytes()? }),
        "structured" => {
            let fields_arr = get_arr(obj, "fields").ok_or("structured missing fields")?;
            let mut fields = Vec::new();
            for fv in fields_arr {
                match fv {
                    Value::Obj(fobj) => {
                        let name = get_str(fobj, "name").ok_or("field missing name")?;
                        let shape_val = get_val(fobj, "shape").ok_or("field missing shape")?;
                        let shape = parse_shape_value(shape_val)?;
                        fields.push(NamedField { name: name.into(), shape });
                    }
                    _ => return Err("field must be object".into()),
                }
            }
            Ok(Shape::Structured { fields })
        }
        "list" => {
            let item_v = get_val(obj, "item").ok_or("list missing item")?;
            let item = Box::new(parse_shape_value(item_v)?);
            // F2: `max_items` is required; silent default to 0 produced
            // invalid shapes that would later be rejected only at the
            // builder. Fail at parse time instead.
            let max_items = get_num(obj, "max_items")
                .ok_or("list shape missing required `max_items`")?;
            Ok(Shape::List { item, max_items })
        }
        _ => Err(format!("unknown shape type: {:?}", shape_type)),
    }
}

// -- Public API ----

pub fn from_jsonld(s: &str) -> Result<SkillDescriptor, DescriptorBuildError> {
    let (val, _) = parse_value(s).map_err(|e| DescriptorBuildError::ShapeValidationError(e))?;
    let Value::Obj(obj) = val else {
        return Err(DescriptorBuildError::ShapeValidationError("root must be object".into()));
    };

    let name = get_str(&obj, "name").ok_or(DescriptorBuildError::ShapeValidationError(
        "missing name".into(),
    ))?;
    let version = get_str(&obj, "version").ok_or(DescriptorBuildError::ShapeValidationError(
        "missing version".into(),
    ))?;
    let content_hash = get_str(&obj, "content_hash").ok_or(DescriptorBuildError::ShapeValidationError(
        "missing content_hash".into(),
    ))?;

    let input_shape = get_val(&obj, "input_shape")
        .map(parse_shape_value)
        .transpose()
        .map_err(|e| DescriptorBuildError::ShapeValidationError(e))?;
    let output_shape = get_val(&obj, "output_shape")
        .map(parse_shape_value)
        .transpose()
        .map_err(|e| DescriptorBuildError::ShapeValidationError(e))?;

    let mut builder = SkillDescriptor::builder()
        .name(name)
        .version(version)
        .content_hash_hex(content_hash);

    if let Some(is) = input_shape {
        builder = builder.input_shape(is);
    }
    if let Some(os) = output_shape {
        builder = builder.output_shape(os);
    }

    if let Some(effects_arr) = get_arr(&obj, "effects") {
        for ev in effects_arr {
            if let Value::Str(e) = ev {
                let eff = match e.as_str() {
                    "none" => EffectKind::None,
                    "file_read" => EffectKind::FileRead,
                    "file_write" => EffectKind::FileWrite,
                    "web_read" => EffectKind::WebRead,
                    "web_write" => EffectKind::WebWrite,
                    "terminal" => EffectKind::Terminal,
                    "llm" => EffectKind::Llm,
                    _ => continue,
                };
                builder = builder.effect(eff);
            }
        }
    }

    if let Some(refs_arr) = get_arr(&obj, "references") {
        for rv in refs_arr {
            if let Value::Str(r) = rv {
                builder = builder.reference(r.clone());
            }
        }
    }

    builder.build()
}

// -- Tests ----------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn test_descriptor() -> SkillDescriptor {
        SkillDescriptor::builder()
            .name("web-search")
            .version("1.0.0")
            .content_hash_hex(
                "10b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b44710b5b447",
            )
            .input_shape(Shape::String { max_bytes: 256 })
            .output_shape(Shape::List {
                item: Box::new(Shape::String { max_bytes: 4096 }),
                max_items: 100,
            })
            .build()
            .unwrap()
    }

    #[test]
    fn serialize_and_parse() {
        let desc = test_descriptor();
        let jsonld = to_jsonld(&desc);
        let parsed = from_jsonld(&jsonld).expect("parse failed");

        assert_eq!(parsed.name(), desc.name());
        assert_eq!(parsed.version(), desc.version());
        assert_eq!(parsed.content_hash(), desc.content_hash());
        assert_eq!(parsed.input_shape(), desc.input_shape());
        assert_eq!(parsed.output_shape(), desc.output_shape());
    }

    #[test]
    fn parse_with_references() {
        // S4: references are pinned `name@<64-hex>` strings.
        let pinned = format!("web-search@{}", "ff".repeat(32));
        let desc = SkillDescriptor::builder()
            .name("research-tool")
            .version("1.0.0")
            .content_hash_hex("aabbccdd".repeat(8).as_str())
            .input_shape(Shape::String { max_bytes: 512 })
            .output_shape(Shape::String { max_bytes: 2048 })
            .reference(pinned.clone())
            .build()
            .unwrap();

        let jsonld = to_jsonld(&desc);
        let parsed = from_jsonld(&jsonld).expect("parse should succeed");
        assert_eq!(parsed.references(), &[pinned]);
    }
}
