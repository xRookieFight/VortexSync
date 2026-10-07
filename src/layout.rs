//! The mapping between a project and a tree of files, in both directions.
//!
//! Both directions are pure: they take and return a [`Files`] map of paths
//! (relative to the source folder, always `/` separated) to contents. Disk
//! access lives in `disk.rs`, which keeps this part easy to test.
//!
//! Layout, inside the source folder:
//!
//! * top level folders are services, `Lighting.json` holds lighting
//! * `Name.server.luau`, `Name.client.luau` and `Name.luau` are Script,
//!   LocalScript and ModuleScript, with an optional `Name.meta.json`
//! * everything else is `Name.<class>.json`, e.g. `Lava.part.json`
//! * an instance with children is a folder, its own data goes in
//!   `init.server.luau` (and friends) for scripts or `_<class>.json` otherwise
//! * a folder without such a file becomes a Model

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use vortexstudio_mcp::scene::{self, FACES, MATERIALS, SHAPES, SURFACES};
use vortexstudio_mcp::vrtx::{
    AttrValue, Class, Instance, Lighting, PointLight, Project, Script, SpotLight, Texture,
};

use crate::names::{self, Siblings};

pub type Files = BTreeMap<String, Vec<u8>>;
type Result<T> = std::result::Result<T, String>;

pub const LIGHTING_FILE: &str = "Lighting.json";

/// The services every project has, in the order Studio stores them.
pub const SERVICES: [Class; 5] = [
    Class::Workspace,
    Class::Lighting,
    Class::ReplicatedStorage,
    Class::ServerScriptService,
    Class::StarterPlayerScripts,
];

fn script_suffix(class: Class) -> Option<&'static str> {
    match class {
        Class::Script => Some(".server.luau"),
        Class::LocalScript => Some(".client.luau"),
        Class::ModuleScript => Some(".luau"),
        _ => None,
    }
}

/// `part` for Part, `remoteevent` for RemoteEvent and so on.
fn kind_of(class: Class) -> String {
    match class {
        Class::Unknown(_) => "unknown".into(),
        c => c.to_string().to_lowercase(),
    }
}

fn class_of_kind(kind: &str) -> Option<Class> {
    Class::ALL.iter().copied().find(|c| kind_of(*c) == kind)
}

// ---------- the JSON shape of one instance ----------

#[derive(Debug, Default, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct InstanceFile {
    /// Only written when the file name can't carry the real name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Class number for classes newer than this tool.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub class_id: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub position: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[f32; 3]>,
    /// Degrees, X then Y then Z.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rotation: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<ColorValue>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transparency: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub material: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub anchored: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub can_collide: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cast_shadow: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spawn_location: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseplate: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub truss: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub custom_appearance: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub velocity: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angular_velocity: Option<[f32; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub textures: Option<Vec<TextureFile>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_light: Option<LightFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_light: Option<LightFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<u64>,

    /// Light data stored on the instance itself, used by PointLight and
    /// SpotLight objects rather than parts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub point_light_data: Option<LightFile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spot_light_data: Option<LightFile>,

    /// Scripts only, in `.meta.json`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub attributes: Option<Map<String, Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub editor_collapsed: Option<bool>,
}

/// Hex when that's exact, raw floats when it isn't, so nothing drifts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum ColorValue {
    Hex(String),
    Rgb([f32; 3]),
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct TextureFile {
    pub face: String,
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LightFile {
    pub color: ColorValue,
    pub brightness: f32,
    pub range: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub angle: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub face: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LightingFile {
    pub ambient_color: ColorValue,
    pub brightness: f32,
    pub sun_color: ColorValue,
    pub sun_illuminance: f32,
    pub sun_shadows: bool,
    /// Degrees, X then Y then Z.
    pub sun_rotation: [f32; 3],
}

fn round4(x: f32) -> f32 {
    ((x as f64 * 10_000.0).round() / 10_000.0) as f32
}

fn color_out(rgb: [f32; 3]) -> ColorValue {
    let hex = scene::hex([rgb[0], rgb[1], rgb[2], 1.0]);
    match scene::parse_hex(&hex) {
        Ok(back) if back == rgb => ColorValue::Hex(hex),
        _ => ColorValue::Rgb(rgb),
    }
}

fn color_in(c: &ColorValue, at: &str) -> Result<[f32; 3]> {
    match c {
        ColorValue::Hex(h) => scene::parse_hex(h).map_err(|e| format!("{at}: {e}")),
        ColorValue::Rgb(v) => Ok(*v),
    }
}

fn rgb(c: [f32; 4]) -> [f32; 3] {
    [c[0], c[1], c[2]]
}

fn enum_name(table: &[&str], id: u32, what: &str) -> Result<String> {
    table
        .get(id as usize)
        .map(|s| s.to_string())
        .ok_or_else(|| {
            format!("unknown {what} id {id}, this version of VortexSync is too old for the project")
        })
}

fn enum_id(table: &[&str], name: &str, what: &str, at: &str) -> Result<u32> {
    table
        .iter()
        .position(|n| n.eq_ignore_ascii_case(name))
        .map(|i| i as u32)
        .ok_or_else(|| {
            format!(
                "{at}: unknown {what} {name:?}, expected one of {}",
                table.join(", ")
            )
        })
}

fn rotation_out(q: [f32; 4]) -> Option<[f32; 3]> {
    if q == [0.0, 0.0, 0.0, 1.0] {
        return None;
    }
    // -0 reads oddly in diffs
    Some(scene::quat_to_euler_deg(q).map(|d| round4(d) + 0.0))
}

fn point_light_out(l: &PointLight) -> LightFile {
    LightFile {
        color: color_out(rgb(l.color)),
        brightness: l.intensity,
        range: l.range,
        angle: None,
        face: None,
    }
}

fn spot_light_out(l: &SpotLight) -> Result<LightFile> {
    Ok(LightFile {
        color: color_out(rgb(l.color)),
        brightness: l.intensity,
        range: l.range,
        angle: Some(l.angle),
        face: Some(enum_name(&FACES, l.face, "face")?),
    })
}

fn light_color(f: &LightFile, at: &str) -> Result<[f32; 4]> {
    let [r, g, b] = color_in(&f.color, at)?;
    Ok([r, g, b, 1.0])
}

fn point_light_in(f: &LightFile, at: &str) -> Result<PointLight> {
    if f.angle.is_some() || f.face.is_some() {
        return Err(format!("{at}: point lights have no angle or face"));
    }
    Ok(PointLight {
        color: light_color(f, at)?,
        intensity: f.brightness,
        range: f.range,
    })
}

fn spot_light_in(f: &LightFile, at: &str) -> Result<SpotLight> {
    Ok(SpotLight {
        color: light_color(f, at)?,
        intensity: f.brightness,
        range: f.range,
        angle: f
            .angle
            .ok_or_else(|| format!("{at}: spot light needs an angle"))?,
        face: enum_id(&FACES, f.face.as_deref().unwrap_or("Front"), "face", at)?,
    })
}

fn attrs_out(attrs: &[(String, AttrValue)]) -> Option<Map<String, Value>> {
    if attrs.is_empty() {
        return None;
    }
    let mut m = Map::new();
    for (k, v) in attrs {
        let json = match v {
            AttrValue::Bool(b) => Value::Bool(*b),
            AttrValue::F32(x) => serde_json::to_value(x).unwrap_or(Value::Null),
            AttrValue::Text(s) => Value::String(s.clone()),
            AttrValue::Vec3(v) => serde_json::json!({ "vector3": v }),
            AttrValue::Color(c) if c[3] == 1.0 => {
                serde_json::json!({ "color": color_out(rgb(*c)) })
            }
            AttrValue::Color(c) => serde_json::json!({ "rgba": c }),
        };
        m.insert(k.clone(), json);
    }
    Some(m)
}

fn attrs_in(m: &Map<String, Value>, at: &str) -> Result<Vec<(String, AttrValue)>> {
    let bad = |k: &str| {
        format!(
            "{at}: attribute {k} must be a boolean, number, string, {{\"vector3\": [x, y, z]}} or {{\"color\": \"#RRGGBB\"}}"
        )
    };
    m.iter()
        .map(|(k, v)| {
            let value = match v {
                Value::Bool(b) => AttrValue::Bool(*b),
                Value::Number(n) => AttrValue::F32(n.as_f64().ok_or_else(|| bad(k))? as f32),
                Value::String(s) => AttrValue::Text(s.clone()),
                Value::Object(o) if o.len() == 1 => {
                    let (tag, inner) = o.iter().next().unwrap();
                    match tag.as_str() {
                        "vector3" => AttrValue::Vec3(
                            serde_json::from_value(inner.clone()).map_err(|_| bad(k))?,
                        ),
                        "rgba" => AttrValue::Color(
                            serde_json::from_value(inner.clone()).map_err(|_| bad(k))?,
                        ),
                        "color" => {
                            let c: ColorValue =
                                serde_json::from_value(inner.clone()).map_err(|_| bad(k))?;
                            let [r, g, b] = color_in(&c, at)?;
                            AttrValue::Color([r, g, b, 1.0])
                        }
                        _ => return Err(bad(k)),
                    }
                }
                _ => return Err(bad(k)),
            };
            Ok((k.clone(), value))
        })
        .collect()
}

/// Everything about an instance except its script source, children and place in the tree.
fn instance_out(inst: &Instance, file_name_says: &str) -> Result<InstanceFile> {
    let mut f = InstanceFile {
        name: (inst.name != file_name_says).then(|| inst.name.clone()),
        class_id: match inst.class {
            Class::Unknown(t) => Some(t),
            _ => None,
        },
        point_light_data: inst.point_light.as_ref().map(point_light_out),
        spot_light_data: inst.spot_light.as_ref().map(spot_light_out).transpose()?,
        enabled: inst
            .script
            .as_ref()
            .and_then(|s| (!s.enabled).then_some(false)),
        attributes: attrs_out(&inst.attributes),
        // Studio saves Models collapsed, so only the unusual case is worth a line
        editor_collapsed: match inst.class {
            Class::Model => (!inst.editor_collapsed).then_some(false),
            _ => inst.editor_collapsed.then_some(true),
        },
        ..Default::default()
    };
    if let Some(p) = &inst.part {
        let d = scene::default_part(&p.name);
        let differs = |a: bool, b: bool| (a != b).then_some(a);
        f.position = Some(p.position);
        f.size = Some(p.scale);
        f.rotation = rotation_out(p.rotation);
        f.color = (rgb(p.color) != rgb(d.color)).then(|| color_out(rgb(p.color)));
        f.transparency = (p.color[3] != 1.0).then(|| round4(1.0 - p.color[3]));
        f.material = (p.material != d.material)
            .then(|| enum_name(&MATERIALS, p.material, "material"))
            .transpose()?;
        f.shape = (p.shape != d.shape)
            .then(|| enum_name(&SHAPES, p.shape, "shape"))
            .transpose()?;
        f.anchored = differs(p.anchored, d.anchored);
        f.can_collide = differs(p.can_collide, d.can_collide);
        f.cast_shadow = differs(p.cast_shadow, d.cast_shadow);
        f.spawn_location = differs(p.spawn_location, d.spawn_location);
        f.baseplate = differs(p.baseplate, d.baseplate);
        f.truss = differs(p.truss, d.truss);
        f.custom_appearance = differs(p.custom_appearance, d.custom_appearance);
        f.velocity = (p.velocity != [0.0; 3]).then_some(p.velocity);
        f.angular_velocity = (p.angular_velocity != [0.0; 3]).then_some(p.angular_velocity);
        if !p.textures.is_empty() {
            f.textures = Some(
                p.textures
                    .iter()
                    .map(|t| {
                        Ok(TextureFile {
                            face: enum_name(&FACES, t.face, "face")?,
                            kind: enum_name(&SURFACES, t.kind, "texture kind")?,
                        })
                    })
                    .collect::<Result<_>>()?,
            );
        }
        f.point_light = p.point_light.as_ref().map(point_light_out);
        f.spot_light = p.spot_light.as_ref().map(spot_light_out).transpose()?;
        f.group = p.group;
        // the part keeps its own copy of the name, usually identical
        if p.name != inst.name {
            return Err(format!(
                "{:?} has part data named {:?}, rename it in Studio so they match",
                inst.name, p.name
            ));
        }
    }
    Ok(f)
}

fn has_part_fields(f: &InstanceFile) -> bool {
    f.position.is_some()
        || f.size.is_some()
        || f.rotation.is_some()
        || f.color.is_some()
        || f.transparency.is_some()
        || f.material.is_some()
        || f.shape.is_some()
        || f.anchored.is_some()
        || f.can_collide.is_some()
        || f.cast_shadow.is_some()
        || f.spawn_location.is_some()
        || f.baseplate.is_some()
        || f.truss.is_some()
        || f.custom_appearance.is_some()
        || f.velocity.is_some()
        || f.angular_velocity.is_some()
        || f.textures.is_some()
        || f.point_light.is_some()
        || f.spot_light.is_some()
        || f.group.is_some()
}

fn instance_in(f: &InstanceFile, class: Class, name: String, at: &str) -> Result<Instance> {
    let finite = |what: &str, v: &[f32]| {
        if v.iter().all(|x| x.is_finite()) {
            Ok(())
        } else {
            Err(format!("{at}: {what} must be finite numbers"))
        }
    };
    let part = if class == Class::Part {
        let mut p = scene::default_part(&name);
        if let Some(v) = f.position {
            finite("position", &v)?;
            p.position = v;
        }
        if let Some(v) = f.size {
            finite("size", &v)?;
            if v.iter().any(|x| *x <= 0.0) {
                return Err(format!("{at}: size values must be positive"));
            }
            p.scale = v;
        }
        if let Some(v) = f.rotation {
            finite("rotation", &v)?;
            p.rotation = scene::euler_deg_to_quat(v);
        }
        if let Some(c) = &f.color {
            let [r, g, b] = color_in(c, at)?;
            p.color = [r, g, b, p.color[3]];
        }
        if let Some(t) = f.transparency {
            if !(0.0..=1.0).contains(&t) {
                return Err(format!("{at}: transparency must be between 0 and 1"));
            }
            p.color[3] = 1.0 - t;
        }
        if let Some(m) = &f.material {
            p.material = enum_id(&MATERIALS, m, "material", at)?;
        }
        if let Some(s) = &f.shape {
            p.shape = enum_id(&SHAPES, s, "shape", at)?;
        }
        for (src, dst) in [
            (f.anchored, &mut p.anchored),
            (f.can_collide, &mut p.can_collide),
            (f.cast_shadow, &mut p.cast_shadow),
            (f.spawn_location, &mut p.spawn_location),
            (f.baseplate, &mut p.baseplate),
            (f.truss, &mut p.truss),
            (f.custom_appearance, &mut p.custom_appearance),
        ] {
            if let Some(b) = src {
                *dst = b;
            }
        }
        if let Some(v) = f.velocity {
            finite("velocity", &v)?;
            p.velocity = v;
        }
        if let Some(v) = f.angular_velocity {
            finite("angular_velocity", &v)?;
            p.angular_velocity = v;
        }
        if let Some(list) = &f.textures {
            p.textures = list
                .iter()
                .map(|t| {
                    Ok(Texture {
                        face: enum_id(&FACES, &t.face, "face", at)?,
                        kind: enum_id(&SURFACES, &t.kind, "texture kind", at)?,
                    })
                })
                .collect::<Result<_>>()?;
        }
        p.point_light = f
            .point_light
            .as_ref()
            .map(|l| point_light_in(l, at))
            .transpose()?;
        p.spot_light = f
            .spot_light
            .as_ref()
            .map(|l| spot_light_in(l, at))
            .transpose()?;
        p.group = f.group;
        Some(p)
    } else {
        if has_part_fields(f) {
            return Err(format!(
                "{at}: only parts have position, size, color and the other part properties"
            ));
        }
        None
    };
    if f.enabled.is_some() && !class.is_script() {
        return Err(format!("{at}: only scripts can be enabled or disabled"));
    }
    Ok(Instance {
        class,
        name,
        parent: None,
        part,
        point_light: f
            .point_light_data
            .as_ref()
            .map(|l| point_light_in(l, at))
            .transpose()?,
        spot_light: f
            .spot_light_data
            .as_ref()
            .map(|l| spot_light_in(l, at))
            .transpose()?,
        script: None,
        attributes: f
            .attributes
            .as_ref()
            .map(|m| attrs_in(m, at))
            .transpose()?
            .unwrap_or_default(),
        editor_collapsed: f.editor_collapsed.unwrap_or(class == Class::Model),
    })
}

fn to_json<T: Serialize>(v: &T) -> Vec<u8> {
    let pretty = serde_json::to_string_pretty(v).expect("plain data always serializes");
    let mut s = tidy_json(&pretty);
    s.push('\n');
    s.into_bytes()
}

/// Puts arrays of plain values on one line (`[0, 1, 10]`) and writes whole
/// numbers without `.0`, which keeps files short and diffs to the point.
/// String contents are copied untouched.
pub fn tidy_json(pretty: &str) -> String {
    let chars: Vec<char> = pretty.chars().collect();
    let mut out = String::with_capacity(pretty.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            let end = string_end(&chars, i);
            out.extend(&chars[i..end]);
            i = end;
        } else if c == '[' {
            match flat_array_end(&chars, i) {
                Some(end) => {
                    out.push('[');
                    let mut items = Vec::new();
                    let mut j = i + 1;
                    while j < end {
                        if chars[j].is_whitespace() || chars[j] == ',' {
                            j += 1;
                        } else if chars[j] == '"' {
                            let e = string_end(&chars, j);
                            items.push(chars[j..e].iter().collect::<String>());
                            j = e;
                        } else {
                            let start = j;
                            while j < end && !chars[j].is_whitespace() && chars[j] != ',' {
                                j += 1;
                            }
                            items.push(trim_zero(&chars[start..j].iter().collect::<String>()));
                        }
                    }
                    out.push_str(&items.join(", "));
                    out.push(']');
                    i = end + 1;
                }
                None => {
                    out.push(c);
                    i += 1;
                }
            }
        } else if c == '-' || c.is_ascii_digit() {
            let start = i;
            while i < chars.len()
                && (chars[i].is_ascii_alphanumeric() || matches!(chars[i], '.' | '-' | '+'))
            {
                i += 1;
            }
            out.push_str(&trim_zero(&chars[start..i].iter().collect::<String>()));
        } else {
            out.push(c);
            i += 1;
        }
    }
    out
}

// index just past the closing quote of the string starting at `start`
fn string_end(chars: &[char], start: usize) -> usize {
    let mut j = start + 1;
    while j < chars.len() {
        match chars[j] {
            '\\' => j += 2,
            '"' => return j + 1,
            _ => j += 1,
        }
    }
    chars.len()
}

// the matching `]` when the array holds no nested arrays or objects
fn flat_array_end(chars: &[char], start: usize) -> Option<usize> {
    let mut j = start + 1;
    while j < chars.len() {
        match chars[j] {
            '"' => j = string_end(chars, j),
            '[' | '{' => return None,
            ']' => return Some(j),
            _ => j += 1,
        }
    }
    None
}

fn trim_zero(num: &str) -> String {
    match num.strip_suffix(".0") {
        Some(int)
            if !int.is_empty()
                && int
                    .trim_start_matches('-')
                    .bytes()
                    .all(|b| b.is_ascii_digit()) =>
        {
            int.to_string()
        }
        _ => num.to_string(),
    }
}

fn lighting_out(l: &Lighting) -> LightingFile {
    LightingFile {
        ambient_color: color_out(rgb(l.ambient_color)),
        brightness: l.brightness,
        sun_color: color_out(rgb(l.sun_color)),
        sun_illuminance: l.sun_illuminance,
        sun_shadows: l.sun_shadow_maps_enabled,
        sun_rotation: scene::quat_to_euler_deg(l.sun_rotation).map(|d| round4(d) + 0.0),
    }
}

fn lighting_in(f: &LightingFile) -> Result<Lighting> {
    let at = LIGHTING_FILE;
    let [ar, ag, ab] = color_in(&f.ambient_color, at)?;
    let [sr, sg, sb] = color_in(&f.sun_color, at)?;
    Ok(Lighting {
        ambient_color: [ar, ag, ab, 1.0],
        brightness: f.brightness,
        sun_color: [sr, sg, sb, 1.0],
        sun_illuminance: f.sun_illuminance,
        sun_shadow_maps_enabled: f.sun_shadows,
        sun_rotation: scene::euler_deg_to_quat(f.sun_rotation),
    })
}

// ---------- project -> files ----------

/// Lays a project out as files. Fails only on things the layout can't hold
/// losslessly, with a message saying what to change in Studio.
pub fn extract(p: &Project) -> Result<Files> {
    let mut files = Files::new();
    let mut kids: HashMap<Option<u64>, Vec<usize>> = HashMap::new();
    for (i, inst) in p.instances.iter().enumerate() {
        kids.entry(inst.parent).or_default().push(i);
    }
    let children = |i: usize| kids.get(&Some(i as u64)).cloned().unwrap_or_default();

    files.insert(LIGHTING_FILE.into(), to_json(&lighting_out(&p.lighting)));

    let mut roots = Siblings::default();
    for &i in kids.get(&None).map(Vec::as_slice).unwrap_or(&[]) {
        let inst = &p.instances[i];
        if !SERVICES.contains(&inst.class) {
            return Err(format!(
                "{:?} ({}) sits at the top level, only services can. Move it into Workspace in Studio",
                inst.name, inst.class
            ));
        }
        if inst.name != inst.class.to_string() {
            return Err(format!(
                "the {} service was renamed to {:?}, rename it back in Studio",
                inst.class, inst.name
            ));
        }
        let dir = roots.claim(&inst.name);
        let own = instance_out(inst, &inst.name)?;
        if own != InstanceFile::default() {
            files.insert(format!("{dir}/_service.json"), to_json(&own));
        }
        let mut siblings = Siblings::default();
        for c in children(i) {
            emit(p, c, &dir, &mut siblings, &children, &mut files)?;
        }
    }
    Ok(files)
}

fn emit(
    p: &Project,
    i: usize,
    dir: &str,
    siblings: &mut Siblings,
    children: &dyn Fn(usize) -> Vec<usize>,
    files: &mut Files,
) -> Result<()> {
    let inst = &p.instances[i];
    let stem = siblings.claim(&inst.name);
    let said = names::name_from_stem(&stem).unwrap_or_default();
    let own = instance_out(inst, &said)?;
    let kids = children(i);

    if inst.class.is_script() != inst.script.is_some() {
        return Err(format!(
            "{:?} is a {} but its script data doesn't match, open and save it in Studio",
            inst.name, inst.class
        ));
    }

    let (data_path, child_dir) = if kids.is_empty() {
        (None, None)
    } else {
        (Some(format!("{dir}/{stem}")), Some(format!("{dir}/{stem}")))
    };

    match (script_suffix(inst.class), &inst.script) {
        (Some(suffix), Some(script)) => {
            let (code, meta) = match &data_path {
                Some(d) => (format!("{d}/init{suffix}"), format!("{d}/init.meta.json")),
                None => (
                    format!("{dir}/{stem}{suffix}"),
                    format!("{dir}/{stem}.meta.json"),
                ),
            };
            files.insert(code, script.source.clone().into_bytes());
            if own != InstanceFile::default() {
                files.insert(meta, to_json(&own));
            }
        }
        _ => {
            let kind = kind_of(inst.class);
            let path = match &data_path {
                Some(d) => format!("{d}/_{kind}.json"),
                None => format!("{dir}/{stem}.{kind}.json"),
            };
            files.insert(path, to_json(&own));
        }
    }

    if let Some(d) = child_dir {
        let mut inner = Siblings::default();
        // reserve the names our own files use inside the folder
        for reserved in ["init", "_service"] {
            inner.claim(reserved);
        }
        for c in kids {
            emit(p, c, &d, &mut inner, children, files)?;
        }
    }
    Ok(())
}

// ---------- files -> project ----------

#[derive(Default, Debug)]
struct Dir {
    files: BTreeMap<String, Vec<u8>>,
    dirs: BTreeMap<String, Dir>,
}

fn tree_of(files: &Files) -> Result<Dir> {
    let mut root = Dir::default();
    for (path, content) in files {
        let mut parts: Vec<&str> = path.split('/').collect();
        let file = parts.pop().ok_or("empty path")?;
        let mut node = &mut root;
        for p in parts {
            node = node.dirs.entry(p.to_string()).or_default();
        }
        node.files.insert(file.to_string(), content.clone());
    }
    Ok(root)
}

enum Entry<'a> {
    Script {
        class: Class,
        source: &'a [u8],
        meta: Option<&'a [u8]>,
    },
    Data {
        class: Class,
        json: &'a [u8],
    },
    Folder {
        dir: &'a Dir,
    },
}

fn script_class_of(file: &str) -> Option<(&str, Class)> {
    for (suffix, class) in [
        (".server.luau", Class::Script),
        (".server.lua", Class::Script),
        (".client.luau", Class::LocalScript),
        (".client.lua", Class::LocalScript),
        (".luau", Class::ModuleScript),
        (".lua", Class::ModuleScript),
    ] {
        if let Some(stem) = file.strip_suffix(suffix) {
            return Some((stem, class));
        }
    }
    None
}

fn parse_json<T: for<'de> Deserialize<'de>>(bytes: &[u8], at: &str) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|e| format!("{at}: {e}"))
}

/// Builds a project from files. `project_id` comes from the project config.
pub fn build(files: &Files, project_id: Option<String>) -> Result<Project> {
    let root = tree_of(files)?;
    let lighting = match root.files.get(LIGHTING_FILE) {
        Some(bytes) => lighting_in(&parse_json(bytes, LIGHTING_FILE)?)?,
        None => scene::new_project(String::new()).lighting,
    };
    for f in root.files.keys() {
        let ours = f.ends_with(".json") || f.ends_with(".luau") || f.ends_with(".lua");
        if ours && f != LIGHTING_FILE {
            return Err(format!(
                "{f} is at the top of the source folder, where only service folders and {LIGHTING_FILE} belong"
            ));
        }
    }
    for d in root.dirs.keys() {
        if !SERVICES
            .iter()
            .any(|s| names::name_from_stem(d).as_deref() == Some(&s.to_string()))
        {
            return Err(format!(
                "top level folder {d:?} isn't a service. Use one of {}",
                SERVICES.map(|s| s.to_string()).join(", ")
            ));
        }
    }

    let mut instances = Vec::new();
    let mut service_dirs = Vec::new();
    for class in SERVICES {
        let name = class.to_string();
        let dir = root.dirs.get(&names::encode(&name));
        let mut inst = match dir.and_then(|d| d.files.get("_service.json")) {
            Some(bytes) => {
                let at = format!("{name}/_service.json");
                let f: InstanceFile = parse_json(bytes, &at)?;
                instance_in(&f, class, name.clone(), &at)?
            }
            None => instance_in(&InstanceFile::default(), class, name.clone(), &name)?,
        };
        inst.parent = None;
        instances.push(inst);
        service_dirs.push(dir);
    }
    for (i, dir) in service_dirs.into_iter().enumerate() {
        if let Some(d) = dir {
            let at = SERVICES[i].to_string();
            add_children(d, i, &at, &mut instances, &["_service.json".to_string()])?;
        }
    }
    Ok(Project {
        version: vortexstudio_mcp::vrtx::CURRENT_VERSION,
        project_id,
        instances,
        lighting,
    })
}

fn entries<'a>(dir: &'a Dir, at: &str, skip: &[String]) -> Result<Vec<(String, Entry<'a>)>> {
    let mut out: BTreeMap<String, Entry<'a>> = BTreeMap::new();
    let mut metas: BTreeMap<String, &[u8]> = BTreeMap::new();
    let add = |stem: &str, e: Entry<'a>, out: &mut BTreeMap<String, Entry<'a>>| {
        if out.insert(stem.to_string(), e).is_some() {
            return Err(format!(
                "{at}/{stem}: two files describe the same instance, keep one"
            ));
        }
        Ok(())
    };
    for (file, bytes) in &dir.files {
        if skip.contains(file) || file.starts_with('.') {
            continue;
        }
        if let Some(stem) = file.strip_suffix(".meta.json") {
            metas.insert(stem.to_string(), bytes);
        } else if let Some((stem, class)) = script_class_of(file) {
            add(
                stem,
                Entry::Script {
                    class,
                    source: bytes,
                    meta: None,
                },
                &mut out,
            )?;
        } else if let Some(rest) = file.strip_suffix(".json") {
            let (stem, kind) = rest.rsplit_once('.').ok_or_else(|| {
                format!("{at}/{file}: expected Name.<class>.json, like Lava.part.json")
            })?;
            let class = class_of_kind(kind)
                .or_else(|| (kind == "unknown").then_some(Class::Unknown(u32::MAX)));
            let class = class
                .ok_or_else(|| format!("{at}/{file}: {kind:?} isn't a class VortexSync knows"))?;
            add(stem, Entry::Data { class, json: bytes }, &mut out)?;
        }
        // anything else (README.md, images, ...) is left alone
    }
    for (name, sub) in &dir.dirs {
        if name.starts_with('.') {
            continue;
        }
        add(name, Entry::Folder { dir: sub }, &mut out)?;
    }
    for (stem, meta) in metas {
        match out.get_mut(&stem) {
            Some(Entry::Script { meta: slot, .. }) => *slot = Some(meta),
            _ => return Err(format!("{at}/{stem}.meta.json has no script next to it")),
        }
    }
    Ok(out.into_iter().collect())
}

fn add_children(
    dir: &Dir,
    parent: usize,
    at: &str,
    out: &mut Vec<Instance>,
    skip: &[String],
) -> Result<()> {
    for (stem, entry) in entries(dir, at, skip)? {
        let here = format!("{at}/{stem}");
        let default_name = names::name_from_stem(&stem)
            .ok_or_else(|| format!("{here}: broken % escape in the file name"))?;
        match entry {
            Entry::Script {
                class,
                source,
                meta,
            } => {
                let inst = script_instance(class, source, meta, default_name, &here)?;
                push(out, inst, parent);
            }
            Entry::Data { class, json } => {
                let inst = data_instance(class, json, default_name, &here)?;
                push(out, inst, parent);
            }
            Entry::Folder { dir: sub } => {
                let (inst, own_files) = folder_instance(sub, default_name, &here)?;
                let idx = push(out, inst, parent);
                add_children(sub, idx, &here, out, &own_files)?;
            }
        }
    }
    Ok(())
}

fn push(out: &mut Vec<Instance>, mut inst: Instance, parent: usize) -> usize {
    inst.parent = Some(parent as u64);
    out.push(inst);
    out.len() - 1
}

fn script_instance(
    class: Class,
    source: &[u8],
    meta: Option<&[u8]>,
    name: String,
    at: &str,
) -> Result<Instance> {
    let source = String::from_utf8(source.to_vec())
        .map_err(|_| format!("{at}: script isn't valid UTF-8"))?;
    let f: InstanceFile = match meta {
        Some(m) => parse_json(m, &format!("{at}.meta.json"))?,
        None => InstanceFile::default(),
    };
    let name = f.name.clone().unwrap_or(name);
    let mut inst = instance_in(&f, class, name, at)?;
    inst.script = Some(Script {
        source,
        enabled: f.enabled.unwrap_or(true),
    });
    Ok(inst)
}

fn data_instance(class: Class, json: &[u8], name: String, at: &str) -> Result<Instance> {
    let f: InstanceFile = parse_json(json, at)?;
    let class = match class {
        Class::Unknown(_) => Class::from_tag(
            f.class_id
                .ok_or_else(|| format!("{at}: unknown class files need a class_id"))?,
        ),
        c => {
            if f.class_id.is_some() {
                return Err(format!(
                    "{at}: class_id only belongs in .unknown.json files"
                ));
            }
            c
        }
    };
    let name = f.name.clone().unwrap_or(name);
    instance_in(&f, class, name, at)
}

/// A folder's own instance comes from an init script or `_<class>.json` inside
/// it. Returns the instance and the files that described it.
fn folder_instance(dir: &Dir, name: String, at: &str) -> Result<(Instance, Vec<String>)> {
    for (file, class) in [
        ("init.server.luau", Class::Script),
        ("init.server.lua", Class::Script),
        ("init.client.luau", Class::LocalScript),
        ("init.client.lua", Class::LocalScript),
        ("init.luau", Class::ModuleScript),
        ("init.lua", Class::ModuleScript),
    ] {
        if let Some(src) = dir.files.get(file) {
            let meta = dir.files.get("init.meta.json").map(Vec::as_slice);
            let inst = script_instance(class, src, meta, name, &format!("{at}/init"))?;
            return Ok((inst, vec![file.to_string(), "init.meta.json".to_string()]));
        }
    }
    // exactly `_<class>.json`, names of children never start with `_` because it's escaped
    let own: Vec<(&String, &Vec<u8>)> = dir
        .files
        .iter()
        .filter(|(f, _)| {
            f.strip_prefix('_')
                .and_then(|r| r.strip_suffix(".json"))
                .is_some_and(|k| !k.contains('.'))
        })
        .collect();
    match own.as_slice() {
        [] => {
            // a plain folder someone made by hand, a Model is the closest container we can create
            let inst = instance_in(&InstanceFile::default(), Class::Model, name, at)?;
            Ok((inst, vec![]))
        }
        [(file, json)] => {
            let kind = file.trim_start_matches('_').trim_end_matches(".json");
            let class = if kind == "unknown" {
                Class::Unknown(u32::MAX)
            } else {
                class_of_kind(kind).ok_or_else(|| {
                    format!("{at}/{file}: {kind:?} isn't a class VortexSync knows")
                })?
            };
            let inst = data_instance(class, json, name, &format!("{at}/{file}"))?;
            Ok((inst, vec![file.to_string()]))
        }
        many => Err(format!(
            "{at}: more than one _<class>.json ({}), keep one",
            many.iter()
                .map(|(f, _)| f.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use vortexstudio_mcp::vrtx;

    const SHOWCASE: &[u8] = include_bytes!("../tests/fixtures/showcase.vrtx");

    fn text(files: &Files, path: &str) -> String {
        String::from_utf8(
            files
                .get(path)
                .unwrap_or_else(|| {
                    panic!(
                        "missing {path}, have {:?}",
                        files.keys().collect::<Vec<_>>()
                    )
                })
                .clone(),
        )
        .unwrap()
    }

    /// Same scene, ignoring instance order (siblings get sorted) and rounding.
    fn same_scene(a: &Project, b: &Project) {
        fn key(p: &Project) -> Vec<String> {
            let mut v: Vec<String> = (0..p.instances.len())
                .map(|i| {
                    let inst = &p.instances[i];
                    let part = inst.part.as_ref().map(|x| {
                        format!(
                            "{:?} {:?} {} {:?} {} {} {:?}",
                            x.position,
                            x.scale,
                            scene::hex(x.color),
                            (x.color[3] * 1000.0).round(),
                            x.material,
                            x.shape,
                            x.textures
                        )
                    });
                    format!(
                        "{} {} {:?} {:?} {:?} {}",
                        scene::path_of(p, i),
                        inst.class,
                        part,
                        inst.script.as_ref().map(|s| (&s.source, s.enabled)),
                        inst.attributes,
                        inst.editor_collapsed
                    )
                })
                .collect();
            v.sort();
            v
        }
        assert_eq!(key(a), key(b));
    }

    #[test]
    fn tidy_output() {
        let v = serde_json::json!({ "position": [10.0, -2.0, 0.5], "name": "a [1.0, 2.0] b", "list": [{ "x": 1.0 }] });
        let t = tidy_json(&serde_json::to_string_pretty(&v).unwrap());
        assert!(t.contains("\"position\": [10, -2, 0.5]"), "{t}");
        assert!(t.contains("\"name\": \"a [1.0, 2.0] b\""), "{t}");
        assert!(t.contains("\"x\": 1"), "{t}");
        let back: serde_json::Value = serde_json::from_str(&t).unwrap();
        assert_eq!(back["position"][2], 0.5);
    }

    #[test]
    fn showcase_layout() {
        let p = vrtx::decode(SHOWCASE).unwrap();
        let files = extract(&p).unwrap();
        assert!(files.contains_key("Lighting.json"));
        assert!(files.contains_key("Workspace/Baseplate.part.json"));
        assert!(files.contains_key("Workspace/Model/_model.json"));
        assert!(files.contains_key("Workspace/Model/Part.part.json"));
        assert!(files.contains_key("Workspace/Part~2.part.json"));
        assert!(files.contains_key("Workspace/Truss.part.json"));
        assert_eq!(
            text(&files, "ServerScriptService/Script.server.luau"),
            "local test = \"Test\";\nprint(test);"
        );
        assert!(files.contains_key("ReplicatedStorage/LocalScript.client.luau"));
        assert!(files.contains_key("ReplicatedStorage/ModuleScript.luau"));
        assert!(files.contains_key("ReplicatedStorage/RemoteEvent.remoteevent.json"));

        let base = text(&files, "Workspace/Baseplate.part.json");
        assert!(base.contains("\"baseplate\": true"), "{base}");
        assert!(base.contains("\"material\": \"SmoothPlastic\""), "{base}");
        // defaults stay out of the file
        assert!(!base.contains("anchored"), "{base}");
    }

    #[test]
    fn extract_build_keeps_the_scene() {
        let p = vrtx::decode(SHOWCASE).unwrap();
        let built = build(&extract(&p).unwrap(), p.project_id.clone()).unwrap();
        same_scene(&p, &built);
        assert_eq!(
            built.lighting.sun_shadow_maps_enabled,
            p.lighting.sun_shadow_maps_enabled
        );
        // the encoded result is a valid file Studio's format reader accepts
        vrtx::decode(&vrtx::encode(&built).unwrap()).unwrap();
    }

    #[test]
    fn extract_is_idempotent() {
        let p = vrtx::decode(SHOWCASE).unwrap();
        let once = extract(&p).unwrap();
        let twice = extract(&build(&once, p.project_id.clone()).unwrap()).unwrap();
        assert_eq!(once, twice);
    }

    #[test]
    fn rich_instances_roundtrip() {
        let mut p = scene::new_project("id".into());
        let ws = 0;
        let a = scene::create(&mut p, ws, Class::Part, "Weird", None).unwrap();
        // Studio allows names scene::create refuses, set one directly
        p.instances[a].name = "Weird/Name".into();
        p.instances[a].part.as_mut().unwrap().name = "Weird/Name".into();
        let props = scene::Props {
            rotation: Some([10.0, 20.0, 30.0]),
            color: Some("#12AB34".into()),
            transparency: Some(0.3),
            shape: Some("Cylinder".into()),
            point_light: Some(scene::LightInput {
                brightness: Some(2.5),
                ..Default::default()
            }),
            spot_light: Some(scene::LightInput {
                angle: Some(45.0),
                face: Some("Top".into()),
                ..Default::default()
            }),
            attributes: Some(BTreeMap::from([
                ("Speed".to_string(), serde_json::json!(12.5)),
                ("Tag".to_string(), serde_json::json!("x")),
                (
                    "Dir".to_string(),
                    serde_json::json!({ "vector3": [0, 1, 0] }),
                ),
                (
                    "Tint".to_string(),
                    serde_json::json!({ "color": "#FF0000" }),
                ),
            ])),
            ..Default::default()
        };
        scene::set_props(&mut p, a, &props).unwrap();
        // a script with children turns into a folder with an init script
        let s = scene::create(&mut p, 3, Class::Script, "Main", Some("print(1)".into())).unwrap();
        scene::create(&mut p, s, Class::ModuleScript, "Helper", None).unwrap();
        scene::set_props(
            &mut p,
            s,
            &scene::Props {
                enabled: Some(false),
                ..Default::default()
            },
        )
        .unwrap();
        scene::create(&mut p, ws, Class::Part, "Twin", None).unwrap();
        scene::create(&mut p, ws, Class::Part, "Twin", None).unwrap();

        let files = extract(&p).unwrap();
        assert!(files.contains_key("Workspace/Weird%2FName.part.json"));
        assert!(files.contains_key("ServerScriptService/Main/init.server.luau"));
        assert!(
            text(&files, "ServerScriptService/Main/init.meta.json").contains("\"enabled\": false")
        );
        assert!(files.contains_key("ServerScriptService/Main/Helper.luau"));
        assert!(files.contains_key("Workspace/Twin~2.part.json"));

        let built = build(&files, p.project_id.clone()).unwrap();
        same_scene(&p, &built);
        let part = built
            .instances
            .iter()
            .find(|i| i.name == "Weird/Name")
            .unwrap()
            .part
            .clone()
            .unwrap();
        let rot = scene::quat_to_euler_deg(part.rotation);
        assert!((rot[2] - 30.0).abs() < 0.01);
        assert_eq!(part.spot_light.unwrap().angle, 45.0);
        assert_eq!(extract(&built).unwrap(), files);
    }

    #[test]
    fn hand_written_files_build() {
        let mut files = Files::new();
        files.insert("Workspace/Lava.part.json".into(), br##"{ "position": [0, 1, 10], "size": [8, 1, 8], "color": "#FF3300", "material": "Metal" }"##.to_vec());
        files.insert(
            "Workspace/Obby/Step1.part.json".into(),
            br#"{ "position": [0, 2, 0], "size": [4, 1, 4] }"#.to_vec(),
        );
        files.insert(
            "ServerScriptService/Kill.server.lua".into(),
            b"print('hi')".to_vec(),
        );
        files.insert("ReplicatedStorage/README.md".into(), b"ignored".to_vec());
        let p = build(&files, None).unwrap();
        assert_eq!(p.instances.len(), 5 + 4);
        let obby = p.instances.iter().position(|i| i.name == "Obby").unwrap();
        assert_eq!(p.instances[obby].class, Class::Model);
        assert!(
            p.instances
                .iter()
                .all(|i| i.parent.is_none_or(|x| (x as usize) < p.instances.len()))
        );
        // parents always come before children
        for (i, inst) in p.instances.iter().enumerate() {
            assert!(inst.parent.is_none_or(|x| (x as usize) < i));
        }
    }

    #[test]
    fn helpful_errors() {
        let err = |path: &str, body: &str| {
            let mut f = Files::new();
            f.insert(path.into(), body.as_bytes().to_vec());
            build(&f, None).unwrap_err()
        };
        assert!(err("Stuff/a.part.json", "{}").contains("isn't a service"));
        assert!(
            err("Workspace/a.part.json", r##"{ "colour": "#fff" }"##).contains("unknown field")
        );
        assert!(err("Workspace/a.part.json", r#"{ "material": "Neon" }"#).contains("Neon"));
        assert!(err("Workspace/a.gizmo.json", "{}").contains("gizmo"));
        assert!(
            err("Workspace/a.remoteevent.json", r#"{ "size": [1, 1, 1] }"#).contains("only parts")
        );
        assert!(err("Workspace/x.meta.json", "{}").contains("no script"));
        assert!(err("notes.json", "{}").contains("top of the source folder"));
    }
}
