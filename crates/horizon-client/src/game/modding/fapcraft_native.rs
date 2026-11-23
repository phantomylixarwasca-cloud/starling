//! Experimental native, animated GeckoLib/Bedrock entity renderer for Fapcraft.
//! Models/animations are read from the installed `sexmod.hmod`; no second vanilla
//! entity, passenger, item_display or Java runtime is involved in drawing.
//!
//! Locomotion uses LivingRenderState. Original Java game logic and its interaction
//! state machine have *not* been ported; this is the actual renderer bridge.
use mod_api::{BlockEntityDrawContext, EntityRenderState, EntityRenderer, EntityRenderers};
use serde_json::Value;
use std::{collections::HashMap, fs::File, io::Read, path::PathBuf, sync::{Arc, Mutex, OnceLock}, time::Instant};

struct Character { id: &'static str, geo: &'static str, animation: &'static str, alias: &'static str }
const CHARACTERS: [Character; 11] = [
    Character { id: "jenny", geo: "geo/jenny/jennydressed.geo.json", animation: "animations/jenny/jenny.animation.json", alias: "jenny" },
    Character { id: "ellie", geo: "geo/ellie/dressed.geo.json", animation: "animations/ellie/ellie.animation.json", alias: "ellie" },
    Character { id: "bia", geo: "geo/bia/biadressed.geo.json", animation: "animations/bia/bia.animation.json", alias: "bia" },
    Character { id: "slime", geo: "geo/slime/dressed.geo.json", animation: "animations/slime/slime.animation.json", alias: "slime" },
    Character { id: "bee", geo: "geo/bee/bee.geo.json", animation: "animations/bee/bee.animation.json", alias: "bee" },
    Character { id: "luna", geo: "geo/cat/cat.geo.json", animation: "animations/cat/cat.animation.json", alias: "cat" },
    Character { id: "allie", geo: "geo/allie/allie.geo.json", animation: "animations/allie/allie.animation.json", alias: "allie" },
    Character { id: "kobold", geo: "geo/kobold/kobold.geo.json", animation: "animations/kobold/kobold.animation.json", alias: "kobold" },
    Character { id: "goblin", geo: "geo/goblin/goblin.geo.json", animation: "animations/goblin/goblin.animation.json", alias: "goblin" },
    Character { id: "galath", geo: "geo/galath/galath.geo.json", animation: "animations/galath/galath.animation.json", alias: "galath" },
    Character { id: "manglelie", geo: "geo/manglelie/manglelie.geo.json", animation: "animations/manglelie/manglelie.animation.json", alias: "manglelie" },
];

#[derive(Clone, Copy, Debug)]
struct Key { time: f32, value: [f32; 3], easing: Easing }
#[derive(Clone, Copy, Debug)]
enum Easing { Linear, SineIn, SineOut, SineBoth, QuadIn, QuadOut, QuadBoth }
#[derive(Default)]
struct Channels { rotation: Vec<Key>, position: Vec<Key>, scale: Vec<Key> }
struct Clip { duration: f32, bones: HashMap<String, Channels> }
struct BoxGeometry { origin: [f32; 3], size: [f32; 3], inflation: f32, pivot: Option<[f32; 3]>, rotation: [f32; 3], uv: [Option<[f32; 4]>; 6], swap_sides: bool, flip_uv: bool }
struct Bone { name: String, pivot: [f32; 3], rotation: [f32; 3], children: Vec<usize>, cubes: Vec<BoxGeometry> }
struct Rig { sprite: String, bones: Vec<Bone>, roots: Vec<usize>, idle: Option<Clip>, walk: Option<Clip>, run: Option<Clip>, talk: Option<Clip>, headpat: Option<Clip>, actions: HashMap<&'static str, Clip> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SocialAction { Talk, Headpat, Clip(&'static str) }
#[derive(Clone, Copy)]
struct SocialState { action: SocialAction, started: Instant }
static SOCIAL: OnceLock<Mutex<HashMap<u32, SocialState>>> = OnceLock::new();

/// Right-click interaction dispatch (called by item_actions.rs), separated
/// from entity model rendering. Social poses automatically expire.
pub(crate) fn trigger_social(id: u32, clip: &'static str) {
    let actions = SOCIAL.get_or_init(|| Mutex::new(HashMap::new()));
    let mut actions = actions.lock().unwrap_or_else(|p| p.into_inner());
    actions.retain(|_, state| state.started.elapsed().as_secs_f32() < 6.0);
    actions.insert(id, SocialState {
        action: match clip {
            "talk" => SocialAction::Talk,
            "headpat" => SocialAction::Headpat,
            other => SocialAction::Clip(other),
        },
        started: Instant::now(),
    });
}
fn social_pose(id: u32) -> Option<(SocialAction, f32)> {
    let actions = SOCIAL.get()?.lock().unwrap_or_else(|p| p.into_inner());
    let state = actions.get(&id)?;
    let time = state.started.elapsed().as_secs_f32();
    (time < 5.0).then_some((state.action, time))
}

/// GeckoLib does not render every bone present in the .geo.json. The original
/// Fapcraft model classes turn off unused armor/variants, and its renderer
/// draws the `steve` player rig in a separate pass with the player's own skin.
/// Rendering that second rig with a character sprite made giant, incorrectly
/// textured heads and torsos overlap the actual character.
fn hidden_in_default_character_pose(name: &str) -> bool {
    name == "steve"
        || name == "items"
        || name == "weapon"
        || name == "itemRenderer"
        || name.starts_with("armor")
}

/// Bedrock face UVs are in *logical* texture pixels (usually 64x64), not
/// pixels of the 512px/4096px PNG. Native cube UVs use 16 texels per sprite.
/// Do not discard negative UV dimensions: original faces may flip vertically.
fn bedrock_uv(u: f32, v: f32, du: f32, dv: f32, width: f32, height: f32) -> Option<[f32; 4]> {
    if ![u, v, du, dv, width, height].iter().all(|x| x.is_finite()) || width <= 0.0 || height <= 0.0 {
        return None;
    }
    Some([u * 16.0 / width, v * 16.0 / height,
          (u + du) * 16.0 / width, (v + dv) * 16.0 / height])
}

fn triplet(v: Option<&Value>, default: [f32; 3]) -> [f32; 3] {
    let Some(array) = v.and_then(Value::as_array) else { return default };
    if array.len() < 3 { return default; }
    std::array::from_fn(|i| array[i].as_f64().unwrap_or(default[i] as f64) as f32)
}
fn parse_track(value: Option<&Value>) -> Vec<Key> {
    let Some(value) = value else { return Vec::new() };
    fn key(time: f32, raw: &Value) -> Option<Key> {
        let vec = if raw.is_array() { raw } else { raw.get("vector")? };
        let easing = match raw.get("easing").and_then(Value::as_str).unwrap_or("linear") {
            "easeInSine" => Easing::SineIn, "easeOutSine" => Easing::SineOut,
            "easeInOutSine" => Easing::SineBoth, "easeInQuad" => Easing::QuadIn,
            "easeOutQuad" => Easing::QuadOut, "easeInOutQuad" => Easing::QuadBoth,
            _ => Easing::Linear,
        };
        Some(Key { time, value: triplet(Some(vec), [0.0; 3]), easing })
    }
    if value.is_array() || value.get("vector").is_some() {
        return key(0.0, value).into_iter().collect();
    }
    let mut out = Vec::new();
    if let Some(map) = value.as_object() {
        for (t, v) in map {
            if let Ok(t) = t.parse::<f32>() {
                if let Some(key) = key(t, v) { out.push(key) }
            }
        }
    }
    out.sort_by(|a, b| a.time.total_cmp(&b.time));
    out
}
fn clip(source: &Value, alias: &str, suffix: &str) -> Option<Clip> {
    let name = format!("animation.{alias}.{suffix}");
    let v = source.get("animations")?.get(&name)?;
    let duration = v.get("animation_length").and_then(Value::as_f64).unwrap_or(1.0) as f32;
    let mut bones = HashMap::new();
    for (name, data) in v.get("bones")?.as_object()? {
        bones.insert(name.clone(), Channels {
            rotation: parse_track(data.get("rotation")),
            position: parse_track(data.get("position")),
            scale: parse_track(data.get("scale")),
        });
    }
    Some(Clip { duration: duration.max(0.001), bones })
}
fn ease(a: Easing, x: f32) -> f32 {
    match a {
        Easing::Linear => x,
        Easing::SineIn => 1.0 - (x * std::f32::consts::FRAC_PI_2).cos(),
        Easing::SineOut => (x * std::f32::consts::FRAC_PI_2).sin(),
        Easing::SineBoth => -(std::f32::consts::PI * x).cos().mul_add(0.5, -0.5),
        Easing::QuadIn => x * x,
        Easing::QuadOut => x * (2.0 - x),
        Easing::QuadBoth => if x < 0.5 { 2.0*x*x } else { 1.0 - (-2.0*x+2.0).powi(2)*0.5 },
    }
}
fn sample(keys: &[Key], time: f32, default: [f32; 3]) -> [f32; 3] {
    let Some(first) = keys.first() else { return default };
    if keys.len() == 1 || time <= first.time { return first.value; }
    for interval in keys.windows(2) {
        let [a, b] = [interval[0], interval[1]];
        if time <= b.time {
            let t = ease(b.easing, ((time - a.time) / (b.time-a.time).max(0.0001)).clamp(0.0, 1.0));
            return std::array::from_fn(|i| a.value[i] + (b.value[i] - a.value[i]) * t);
        }
    }
    keys.last().map_or(default, |k| k.value)
}
fn load_rig(zip: &mut zip::ZipArchive<File>, character: &Character) -> Result<Rig, String> {
    fn json(zip: &mut zip::ZipArchive<File>, name: &str) -> Result<Value, String> {
        let mut text = String::new();
        zip.by_name(name).map_err(|e| format!("{name}: {e}"))?
            .read_to_string(&mut text).map_err(|e| format!("{name}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))
    }
    let geometry = json(zip, &format!("assets/sexmod/{}", character.geo))?;
    let anim = json(zip, &format!("assets/sexmod/{}", character.animation))?;
    let obj = geometry.get("minecraft:geometry").and_then(Value::as_array)
        .and_then(|a| a.first()).ok_or("No minecraft:geometry in original asset")?;
    let description = obj.get("description").ok_or("Missing geometry description")?;
    let tex_w = description.get("texture_width").and_then(Value::as_f64).unwrap_or(64.0) as f32;
    let tex_h = description.get("texture_height").and_then(Value::as_f64).unwrap_or(64.0) as f32;
    let mut bones = Vec::new();let mut parents = Vec::new();
    for bone in obj.get("bones").and_then(Value::as_array).ok_or("Missing geometry bones")? {
        let name = bone.get("name").and_then(Value::as_str).unwrap_or("").to_owned();
        if name.is_empty() { continue; }
        let mut cubes = Vec::new();
        if let Some(parts) = bone.get("cubes").and_then(Value::as_array) {
            for c in parts {
                let origin = triplet(c.get("origin"), [0.0;3]);
                let size = triplet(c.get("size"), [0.0;3]);
                if size.iter().any(|x| !x.is_finite() || *x < 0.0) { continue; }
                let mut uv = [None;6];
                if let Some(face) = c.get("uv").and_then(Value::as_object) {
                    for (idx, direction) in ["down","up","north","south","west","east"].iter().enumerate() {
                        if let Some(info) = face.get(*direction) {
                            let at = info.get("uv").and_then(Value::as_array);
                            let sz = info.get("uv_size").and_then(Value::as_array);
                            if let (Some(at), Some(sz)) = (at, sz) {
                                let num = |v: &Value| v.as_f64().unwrap_or(0.0) as f32;
                                if at.len() == 2 && sz.len() == 2 {
                                    let (u, v) = (num(&at[0]),num(&at[1]));
                                    let (du,dv) = (num(&sz[0]),num(&sz[1]));
                                    uv[idx] = bedrock_uv(u, v, du, dv, tex_w, tex_h);
                                }
                            }
                        }
                    }
                }
                let bone_mirror = bone.get("mirror").and_then(Value::as_bool).unwrap_or(false);
                let cube_mirror = c.get("mirror").and_then(Value::as_bool).unwrap_or(false);
                cubes.push(BoxGeometry {
                    origin,size,inflation:c.get("inflate").or_else(||bone.get("inflate"))
                        .and_then(Value::as_f64).unwrap_or(0.0) as f32,
                    pivot:c.get("pivot").map(|v| triplet(Some(v),[0.0;3])),
                    rotation:triplet(c.get("rotation"),[0.0;3]),uv,
                    swap_sides:bone_mirror || cube_mirror,
                    flip_uv:cube_mirror,
                });
            }
        }
        parents.push(bone.get("parent").and_then(Value::as_str).map(str::to_owned));
        bones.push(Bone{name,pivot:triplet(bone.get("pivot"),[0.0;3]),
            rotation:triplet(bone.get("rotation"),[0.0;3]),children:Vec::new(),cubes});
    }
    let names: HashMap<String, usize> = bones.iter().enumerate().map(|(i,b)|(b.name.clone(),i)).collect();
    let mut roots=Vec::new();
    for (i,parent) in parents.iter().enumerate(){
        if let Some(p)=parent.as_ref().and_then(|name|names.get(name).copied()).filter(|p|*p!=i) {
            bones[p].children.push(i);
        } else {roots.push(i);}
    }
    // Only named non-explicit clips offered by the character-specific UI are loaded.
    // These are read from each original .animation.json, not synthesized poses.
    let mut actions = HashMap::new();
    for suffix in [
        "wave","fhappy","payment","sit","fortnite",
        "hug","sitdown","sitdownidle","hugidle",
        "headpat","talk_idle2","jumpstart","jumpend",
        "attack0","head_pat","start_fishing","sitting_intro",
        "summon_normal","rich_normal","summon","sleep",
        "blink","jump_1","stand_up","give_coin","boost",
        "controlled_flight","angry_face","running","walk",
    ] {
        if let Some(found) = clip(&anim, character.alias, suffix) {
            actions.insert(suffix, found);
        }
    }
    Ok(Rig { sprite:format!("sexmod:entity/fapcraft_native/{}", character.id),bones,roots,actions,
        idle:clip(&anim, character.alias,"idle")
            .or_else(||clip(&anim, character.alias,"summon_wait"))
            .or_else(||clip(&anim, character.alias,"rich_normal")),
        walk:clip(&anim, character.alias,"walk")
            .or_else(||clip(&anim, character.alias,"running")),
        run:clip(&anim, character.alias,"run")
            .or_else(||clip(&anim, character.alias,"running"))
            .or_else(||clip(&anim, character.alias,"fastwalk")),
        talk:clip(&anim, character.alias,"talk_idle")
            .or_else(||clip(&anim, character.alias,"talk_response"))
            .or_else(||clip(&anim, character.alias,"wave_idle")),
        headpat:clip(&anim, character.alias,"headpat")
            .or_else(||clip(&anim, character.alias,"head_pat")),
    })
}

static RIGS: OnceLock<Vec<Option<Rig>>> = OnceLock::new();
fn rigs() -> &'static Vec<Option<Rig>> {
    RIGS.get_or_init(|| {
        let root = std::env::var_os("HORIZON_HOME").map(PathBuf::from).unwrap_or_default();
        let path = root.join("horizon-mods/sexmod.hmod");
        let attempt = (|| -> Result<Vec<Option<Rig>>, String> {
            let file=File::open(&path).map_err(|e|format!("{}: {e}",path.display()))?;
            let mut archive=zip::ZipArchive::new(file).map_err(|e|e.to_string())?;
            Ok(CHARACTERS.iter().map(|character|{
                match load_rig(&mut archive,character) {
                    Ok(rig)=>Some(rig),
                    Err(error)=>{log::warn!("Fapcraft native renderer {}: {error}",character.id);None}
                }
            }).collect())
        })();
        attempt.unwrap_or_else(|error|{
            log::warn!("Fapcraft native renderer cannot read package: {error}");
            (0..CHARACTERS.len()).map(|_|None).collect()
        })
    })
}
/// Exact axis conversion used by Fapcraft's bundled GeckoLib GeoBuilder.java:
/// rotation X and Y are negated and pivot X is negated when JSON is loaded.
/// Geckolib MatrixStack rotates Z, then Y, then X.
fn rotate(draw: &mut dyn BlockEntityDrawContext, deg: [f32;3], pivot: [f32;3]) {
    let pivot=[-pivot[0]/16.0,pivot[1]/16.0,pivot[2]/16.0];
    for (axis, angle) in [([0.0,0.0,1.0],deg[2]),([0.0,1.0,0.0],-deg[1]),([1.0,0.0,0.0],-deg[0])] {
        if angle.abs()>0.0001 {draw.rotate_about(axis,angle,pivot);}
    }
}

/// Match the six face vertex orders in GeckoLib's bundled GeoCube.java, not
/// Minecraft's block FaceBakery. The latter rotated some original texture
/// faces a quarter-turn (and did not apply the Bedrock flipped X axis).
fn gecko_face(c: &BoxGeometry, direction: usize) -> [[f32;3];4] {
    let o=c.origin;let sz=c.size;let inflate=c.inflation;
    // GeoCube.createFromPojoCube: x = -(origin.x + size.x) / 16.
    let lo=[-(o[0]+sz[0])-inflate,o[1]-inflate,o[2]-inflate].map(|v|v/16.0);
    let hi=[-o[0]+inflate,o[1]+sz[1]+inflate,o[2]+sz[2]+inflate].map(|v|v/16.0);
    let p=[
        [lo[0],lo[1],lo[2]], // P1
        [lo[0],lo[1],hi[2]], // P2
        [lo[0],hi[1],lo[2]], // P3
        [lo[0],hi[1],hi[2]], // P4
        [hi[0],lo[1],lo[2]], // P5
        [hi[0],lo[1],hi[2]], // P6
        [hi[0],hi[1],lo[2]], // P7
        [hi[0],hi[1],hi[2]], // P8
    ];
    // Order: down, up, north, south, west, east.
    let indices = match (direction, c.swap_sides) {
        (0,false)|(1,true) => [0,4,5,1],
        (1,false)|(0,true) => [3,7,6,2],
        (2,_) => [2,6,4,0],
        (3,_) => [7,3,1,5],
        (4,false)|(5,true) => [3,2,0,1],
        (5,false)|(4,true) => [6,7,5,4],
        _ => unreachable!(),
    };
    // GeoQuad.java supplies top-right, top-left, bottom-left, bottom-right.
    // native_draw_quad requires top-left, bottom-left, bottom-right, top-right.
    [p[indices[1]], p[indices[2]], p[indices[3]], p[indices[0]]]
}
fn visit(rig: &Rig, index: usize, clip: Option<&Clip>, time: f32, draw:&mut dyn BlockEntityDrawContext, depth: usize) {
    if depth > 64 {return;}
    let Some(bone)=rig.bones.get(index) else {return;};
    // The Java cv/d_ renderers hide entire alternate bone subtrees. In
    // particular the Steve player-model pass requires a different skin.
    if hidden_in_default_character_pose(&bone.name) { return; }
    draw.push();
    let channels=clip.and_then(|c|c.bones.get(&bone.name));
    if let Some(c)=channels {
        let pos=sample(&c.position,time,[0.0;3]);
        // MatrixStack.translate(GeoBone) negates X, unlike raw Bedrock JSON.
        draw.translate([-pos[0]/16.0,pos[1]/16.0,pos[2]/16.0]);
    }
    let rot=channels.map_or([0.0;3],|c|sample(&c.rotation,time,[0.0;3]));
    rotate(draw,std::array::from_fn(|i|bone.rotation[i]+rot[i]),bone.pivot);
    if let Some(c)=channels {
        if !c.scale.is_empty() {
            let scale=sample(&c.scale,time,[1.0;3]);
            let pivot=[-bone.pivot[0]/16.0,bone.pivot[1]/16.0,bone.pivot[2]/16.0];
            draw.translate(pivot);draw.scale(scale);
            draw.translate(pivot.map(|v|-v));
        }
    }
    for c in &bone.cubes {
        if c.uv.iter().all(Option::is_none){continue;}
        draw.push();
        if c.rotation.iter().any(|v|v.abs()>0.001) {
            rotate(draw,c.rotation,c.pivot.unwrap_or(bone.pivot));
        }
        for (direction, rect) in c.uv.iter().enumerate() {
            let Some([u0,v0,u1,v1]) = rect else {continue;};
            let uv = if c.flip_uv {[*u1,*v0,*u0,*v1]} else {[*u0,*v0,*u1,*v1]};
            let _=draw.quad(&rig.sprite,gecko_face(c,direction),uv,0xFFFFFF);
        }
        draw.pop();
    }
    for child in &bone.children {visit(rig,*child,clip,time,draw,depth+1);}
    draw.pop();
}

/// Starling's 0° body yaw is south; the original GeoRenderer transforms
/// its model from a north-facing rest coordinate system.
fn geckolib_model_yaw(body_yaw: f32) -> f32 {
    (180.0 - body_yaw).rem_euclid(360.0)
}

struct Renderer(usize);
impl EntityRenderer for Renderer {
    fn submit(&self,entity:&EntityRenderState,draw:&mut dyn BlockEntityDrawContext)->Result<(),String> {
        let Some(rig)=rigs().get(self.0).and_then(Option::as_ref) else {
            // When the package isn't installed, the entity type is not present;
            // returning Ok without geometry avoids rendering a wrong vanilla model.
            return Ok(());
        };
        let speed=entity.living.map_or(0.0,|l|l.walk_animation_speed);
        let movement_clip=if speed>0.60 {rig.run.as_ref().or(rig.walk.as_ref())}
            else if speed>0.055 {rig.walk.as_ref().or(rig.run.as_ref())}
            else {rig.idle.as_ref()};
        let social=social_pose(entity.id).and_then(|(action, seconds)|{
            let clip=match action {
                SocialAction::Talk=>rig.talk.as_ref(),
                SocialAction::Headpat=>rig.headpat.as_ref(),
                SocialAction::Clip(name)=>rig.actions.get(name),
            }?;
            Some((clip, seconds.min(clip.duration)))
        });
        let (clip,time)=match social {
            Some((clip,time))=>(Some(clip),time),
            None=>{
                let time=movement_clip.map_or(0.0,|c| (entity.age * 0.05 * if speed>0.65 {1.4} else {1.0}).rem_euclid(c.duration));
                (movement_clip,time)
            }
        };
        draw.push();
        // GeckoLib's MobRenderer pose applies the character's baked
        // 180-degree forward-axis correction. Without it our mesh looked
        // toward the player while its living-entity AI walked away.
        if let Some(living)=entity.living {
            draw.rotate_about([0.0,1.0,0.0],geckolib_model_yaw(living.body_rot),[0.0;3]);
            draw.scale([living.scale,living.scale,living.scale]);
            if living.death_time>0.0 {draw.rotate_about([0.0,0.0,1.0],(living.death_time*4.5).min(90.0),[0.0,0.6,0.0]);}
        } else {draw.rotate_about([0.0,1.0,0.0],geckolib_model_yaw(entity.yaw),[0.0;3]);}
        for root in &rig.roots {visit(rig,*root,clip,time,draw,0);}
        draw.pop();
        Ok(())
    }
}

/// Registers the 11 unique Fapcraft entity types in the core's real living
/// renderer registry; each renderer is used only when that entity type exists.
pub(super) fn install(registry: &mut dyn EntityRenderers) {
    for (index, character) in CHARACTERS.iter().enumerate() {
        registry.register(format!("sexmod:{}",character.id),Arc::new(Renderer(index)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn social_clips_selected_without_changing_locomotion_when_absent() {
        assert_eq!(CHARACTERS.len(),11);
        assert!(social_pose(u32::MAX).is_none());
        trigger_social(u32::MAX, "talk");
        assert_eq!(social_pose(u32::MAX).map(|x|x.0),Some(SocialAction::Talk));
        trigger_social(u32::MAX, "headpat");
        assert_eq!(social_pose(u32::MAX).map(|x|x.0),Some(SocialAction::Headpat));
        trigger_social(u32::MAX, "wave");
        assert_eq!(social_pose(u32::MAX).map(|x|x.0),Some(SocialAction::Clip("wave")));
    }
    #[test]
    fn fapcraft_north_south_facing_regression() {
        assert_eq!(geckolib_model_yaw(0.0),180.0);
        assert_eq!(geckolib_model_yaw(90.0),90.0);
        assert_eq!(geckolib_model_yaw(180.0),0.0);
        assert_eq!(geckolib_model_yaw(-90.0),270.0);
    }
    #[test]
    fn geckolib_faces_and_x_conversion() {
        let cube=BoxGeometry {origin:[1.0,2.0,3.0],size:[2.0,3.0,4.0],inflation:0.0,
            pivot:None,rotation:[0.0;3],uv:[None;6],swap_sides:false,flip_uv:false};
        // GeckoLib reverses X and translates the origin by the cube width.
        let north=gecko_face(&cube,2);
        assert_eq!(north[0],[-1.0/16.0,5.0/16.0,3.0/16.0]);
        assert_eq!(north[2],[-3.0/16.0,2.0/16.0,3.0/16.0]);
        let mut mirrored=cube;
        mirrored.swap_sides=true;
        assert_ne!(gecko_face(&mirrored,4), gecko_face(&mirrored,5));
    }
    #[test]
    fn original_geckolib_visibility_and_uvs() {
        for bone in ["steve", "items", "armorChest", "armorHelmet", "armorBoobs", "weapon"] {
            assert!(hidden_in_default_character_pose(bone), "{bone}");
        }
        for bone in ["body", "torso", "hip", "head", "bra", "slip", "hair", "leaf7"] {
            assert!(!hidden_in_default_character_pose(bone), "{bone}");
        }
        assert_eq!(bedrock_uv(19., 57., 5., 7., 64., 64.), Some([4.75,14.25,6.0,16.0]));
        assert_eq!(bedrock_uv(46., 36., 5., -3., 64., 64.), Some([11.5,9.0,12.75,8.25]));
        assert_eq!(bedrock_uv(0., 0., 1., 1., 0., 64.), None);
    }
    #[test]
    fn easing_and_track_interpolation() {
        let keys=parse_track(Some(&serde_json::json!({"0.0":{"vector":[0,0,0]},"1.0":{"vector":[10,0,0]}})));
        assert!((sample(&keys,0.5,[0.0;3])[0]-5.0).abs()<0.001);
        assert_eq!(CHARACTERS.len(),11);
    }
}