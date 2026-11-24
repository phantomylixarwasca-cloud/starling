//! Experimental native, animated GeckoLib/Bedrock entity renderer for Fapcraft.
//! Models/animations are read from the installed `sexmod.hmod`; no second vanilla
//! entity, passenger, item_display or Java runtime is involved in drawing.
//!
//! Locomotion uses LivingRenderState. Original Java game logic and its interaction
//! state machine have *not* been ported; this is the actual renderer bridge.
use mod_api::{BlockEntityDrawContext, EntityRenderState, EntityRenderer, EntityRenderers};
use horizon_client::mobs::anim::BodyPose;
use serde_json::Value;
use std::{collections::HashMap, fs::File, io::Read, path::PathBuf, sync::{atomic::{AtomicBool,Ordering}, Arc, Mutex, OnceLock}, time::Instant};

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
enum Easing {
    Linear, Step,
    SineIn, SineOut, SineBoth, QuadIn, QuadOut, QuadBoth,
    CubicIn, CubicOut, CubicBoth, QuartIn, QuartOut, QuartBoth,
    QuintIn, QuintOut, QuintBoth, BackIn, BackOut, BackBoth,
    ElasticIn, ElasticOut, ElasticBoth, ExpoIn, ExpoOut, ExpoBoth,
    CircIn, CircOut, BounceIn,
}
#[derive(Default)]
struct Channels { rotation: Vec<Key>, position: Vec<Key>, scale: Vec<Key> }
#[derive(Clone, Debug)]
struct SoundCue { time:f32, effect:String }
struct Clip { duration: f32, bones: HashMap<String, Channels>, sound_cues: Vec<SoundCue> }
#[derive(Clone)]
struct BoxGeometry { origin: [f32; 3], size: [f32; 3], inflation: f32, pivot: Option<[f32; 3]>, rotation: [f32; 3], uv: [Option<[f32; 4]>; 6], swap_sides: bool, flip_uv: bool }
#[derive(Clone)]
struct Bone { name: String, pivot: [f32; 3], rotation: [f32; 3], children: Vec<usize>, cubes: Vec<BoxGeometry> }
struct Rig { sprite: String, bones: Vec<Bone>, roots: Vec<usize>, nude: Option<Box<Rig>>, idle: Option<Clip>, walk: Option<Clip>, run: Option<Clip>, talk: Option<Clip>, headpat: Option<Clip>, actions: HashMap<String, Clip> }

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SocialAction { Talk, Headpat, Clip(&'static str) }
#[derive(Clone, Copy)]
struct SocialState {
    action: SocialAction,
    kind: &'static str,
    started: Instant,
    duration: f32,
    playlist: Option<&'static [&'static str]>,
    phase: usize,
    session_started: Instant,
    sound_clip: Option<&'static str>,
    sound_time: f32,
    sound_counter: u32,
}
static BIA_FAST: AtomicBool = AtomicBool::new(false);
// The real game camera remains user-selectable. On the first-person camera,
// render its hands, not the third-person Steve sub-rig inside Bia.
static BIA_FIRST_PERSON: AtomicBool = AtomicBool::new(true);
/// Pending one-shot camera change; cleared by the Android client tick, not
/// by the integrated server's render-independent state machine.
static BIA_INITIAL_POV: AtomicBool = AtomicBool::new(false);
static BIA_CLIENT_WAS_PAIRED: AtomicBool = AtomicBool::new(false);
pub(crate) fn take_bia_initial_pov()->bool {
    // A server-side trigger can be set before client initialization and was
    // sometimes lost in the mobile build. The client independently detects
    // the first tick of a paired scene; subsequent perspective toggles are
    // never overridden by this one-time request.
    let active=bia_session_active();
    let previous=BIA_CLIENT_WAS_PAIRED.swap(active,Ordering::Relaxed);
    let requested=BIA_INITIAL_POV.swap(false,Ordering::Relaxed);
    active && (!previous || requested)
}
static BIA_JUMP_HELD: AtomicBool = AtomicBool::new(false);
static BIA_FINISH_REQUEST: AtomicBool = AtomicBool::new(false);
pub(crate) fn set_bia_first_person(value: bool) {
    BIA_FIRST_PERSON.store(value, Ordering::Relaxed);
}
/// Use the same Jump control as original Java's two-actor scene. A rising
/// edge requests the source clip's ending; it must never trigger merely
/// because the player was jumping before the interaction started.
pub(crate) fn set_bia_jump(value: bool) {
    let previous=BIA_JUMP_HELD.swap(value,Ordering::Relaxed);
    if value && !previous && bia_session_active() {
        BIA_FINISH_REQUEST.store(true,Ordering::Relaxed);
    }
}
pub(crate) fn bia_session_active() -> bool {
    SOCIAL.get().is_some_and(|actions| {
        actions.lock().unwrap_or_else(|e|e.into_inner())
            .values().any(|state|state.playlist.is_some()
                && state.session_started.elapsed().as_secs()<120)
    })
}
pub(crate) fn set_bia_fast(value: bool) {
    BIA_FAST.store(value, Ordering::Relaxed);
}
const BIA_DOGGY: &[&str] = &[
    "prone_doggy_intro", "prone_doggy_insert", "prone_doggy_soft",
    "prone_doggy_hard1", "prone_doggy_hard2", "prone_doggy_hard3",
    "prone_doggy_cum",
];
const BIA_ANA: &[&str] = &[
    // The bed phase already played anal_prepare, then held anal_wait.
    "anal_start", "anal_slow", "anal_fast", "anal_cum",
];
static SOCIAL: OnceLock<Mutex<HashMap<u32, SocialState>>> = OnceLock::new();
static BIA_UNDRESSED: OnceLock<Mutex<HashMap<u32, bool>>> = OnceLock::new();
static BIA_WARDROBE_PENDING: OnceLock<Mutex<HashMap<u32, (Instant, bool)>>> = OnceLock::new();
/// Forge e0.java only switches geometry after the "becomeNude" sound keyframe
/// in animation.bia.strip, at 3.0 seconds; stripDone is at 5.0 seconds.
pub(crate) fn toggle_bia_dress(id: u32) -> Option<bool> {
    let mut pending = BIA_WARDROBE_PENDING.get_or_init(|| Mutex::new(HashMap::new()))
        .lock().unwrap_or_else(|e| e.into_inner());
    if pending.contains_key(&id) { return None; }
    let dressed = BIA_UNDRESSED.get_or_init(|| Mutex::new(HashMap::new()))
        .lock().unwrap_or_else(|e| e.into_inner());
    let value = !dressed.get(&id).copied().unwrap_or(false);
    pending.insert(id, (Instant::now(), value));
    Some(value)
}
/// All four source characters with a separate wardrobe rig share this
/// per-entity transition timer. Their original strip keyframe switches
/// geometry at 3.0s; no costume toggle is offered to other NPCs.
pub(crate) fn toggle_character_dress(id:u32,kind:&str)->Option<bool> {
    if !matches!(kind,"jenny"|"ellie"|"slime"|"bia") {return None}
    toggle_bia_dress(id)
}
fn bia_undressed(id: u32) -> bool {
    if let Some(pending) = BIA_WARDROBE_PENDING.get() {
        let mut pending = pending.lock().unwrap_or_else(|e|e.into_inner());
        if pending.get(&id).is_some_and(|(started,_)| started.elapsed().as_secs_f32() >= 3.0) {
            if let Some((_,value))=pending.remove(&id) {
                BIA_UNDRESSED.get_or_init(|| Mutex::new(HashMap::new()))
                    .lock().unwrap_or_else(|e|e.into_inner()).insert(id,value);
            }
        }
    }
    BIA_UNDRESSED.get().and_then(|state|
        state.lock().ok().and_then(|m| m.get(&id).copied())).unwrap_or(false)
}

/// Companion orders mirror the five management options of the original
/// Forge m.java / e2.java menu. Orders are runtime-only until the native
/// saved-data adapter is installed; do not present them as persistent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CompanionMode { Follow, Stay, ReturnHome }
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompanionOrder {
    pub mode: CompanionMode,
    pub home: [f64; 3],
}
static COMPANIONS: OnceLock<Mutex<HashMap<u32, CompanionOrder>>> = OnceLock::new();
fn companion_orders() -> &'static Mutex<HashMap<u32, CompanionOrder>> {
    COMPANIONS.get_or_init(|| Mutex::new(HashMap::new()))
}
pub(crate) fn companion_order(id: u32) -> Option<CompanionOrder> {
    companion_orders().lock().unwrap_or_else(|e| e.into_inner()).get(&id).copied()
}
pub(crate) fn set_companion_mode(id: u32, mode: CompanionMode, position: [f64; 3]) {
    let mut data = companion_orders().lock().unwrap_or_else(|e| e.into_inner());
    let order = data.entry(id).or_insert(CompanionOrder {
        mode: CompanionMode::Stay,
        home: position,
    });
    order.mode = mode;
}
pub(crate) fn set_companion_home(id: u32, position: [f64; 3]) {
    let mut data = companion_orders().lock().unwrap_or_else(|e| e.into_inner());
    let order = data.entry(id).or_insert(CompanionOrder {
        mode: CompanionMode::Stay,
        home: position,
    });
    order.home = position;
    order.mode = CompanionMode::Stay;
}

/// Right-click interaction dispatch (called by item_actions.rs), separated
/// from entity model rendering. Social poses automatically expire.
pub(crate) fn trigger_social(id: u32, kind: &str, clip: &'static str) {
    // Match the original clip's timing, not an arbitrary three-second effect.
    // Long idle/facial clips have a cap to prevent trapping an NPC forever.
    let index = CHARACTERS.iter().position(|character| character.id == kind);
    let duration = index
        .and_then(|index| rigs().get(index))
        .and_then(Option::as_ref)
        .and_then(|rig| match clip {
            "talk" => rig.talk.as_ref(),
            "headpat" => rig.headpat.as_ref(),
            other => rig.actions.get(other),
        })
        .map_or(2.0, |clip| clip.duration.clamp(0.25, 12.0));
    let actions = SOCIAL.get_or_init(|| Mutex::new(HashMap::new()));
    let mut actions = actions.lock().unwrap_or_else(|p| p.into_inner());
    actions.retain(|_, state| state.started.elapsed().as_secs_f32() < state.duration);
    actions.insert(id, SocialState {
        kind: index.map_or("jenny", |index| CHARACTERS[index].id),
        action: match clip {
            "talk" => SocialAction::Talk,
            "headpat" => SocialAction::Headpat,
            other => SocialAction::Clip(other),
        },
        started: Instant::now(),
        duration,
        playlist: None,
        phase: 0,
        session_started: Instant::now(),
        sound_clip: None,
        sound_time: -1.0,
        sound_counter: 0,
    });
}
/// Preview of Bia's original clip sequences; input-driven speed and camera
/// changes from Forge still require a separate native controller.
pub(crate) fn trigger_bia_sequence(id: u32, choice: &str) -> bool {
    let sequence = match choice {
        "doggy" => BIA_DOGGY,
        "ana" => BIA_ANA,
        _ => return false,
    };
    let Some(&first) = sequence.first() else { return false };
    trigger_social(id, "bia", first);
    if let Some(active) = SOCIAL.get() {
        if let Some(state) = active.lock().unwrap_or_else(|e| e.into_inner()).get_mut(&id) {
            state.playlist = Some(sequence);
            // In Java the black-screen handoff begins in POV. Request the
            // camera change in the *client* world rather than the integrated
            // server where it had no effect.
            BIA_INITIAL_POV.store(true,Ordering::Relaxed);
            return true;
        }
    }
    false
}

/// The integrated world server pauses a character's wandering AI only while
/// an actual social clip is playing. Other mobs and the user's saved NoAI
/// setting are left untouched.
pub(crate) fn social_active(id: u32) -> bool {
    social_pose(id).is_some()
}
/// Allow the player to end a paired sequence by interacting with Bia again.
pub(crate) fn stop_bia_sequence(id: u32) -> bool {
    SOCIAL.get().and_then(|states| {
        let mut states=states.lock().unwrap_or_else(|e|e.into_inner());
        if states.get(&id).is_some_and(|state|state.playlist.is_some()) {
            states.remove(&id);
            Some(())
        } else { None }
    }).is_some()
}
fn paired_clip(name: &str) -> bool {
    // Bed preparation and idle must never immobilize the player: they need
    // to walk toward Bia before the paired controller begins.
    (name.starts_with("anal_") && !matches!(name,"anal_prepare"|"anal_wait"))
        || name.starts_with("prone_doggy_")
}
/// Contact interactions temporarily stop player walking, as the original
/// two-actor controller does. The ordinary player movement loop restores its
/// previous movement rules as soon as the animation finishes.
pub(crate) fn paired_action_active() -> bool {
    let Some(active) = SOCIAL.get() else { return false; };
    let active = active.lock().unwrap_or_else(|p| p.into_inner());
    active.values().any(|state| {
        (state.started.elapsed().as_secs_f32() < state.duration
            || (state.playlist.is_some()
                && state.session_started.elapsed().as_secs()<120))
            && (matches!(state.action,
                SocialAction::Headpat |
                SocialAction::Clip("head_pat") |
                SocialAction::Clip("hug"))
                || matches!(state.action, SocialAction::Clip(name) if paired_clip(name)))
    })
}
/// Simple player-half counterpart for non-explicit contact gestures. It
/// animates the *real player model* (including their actual skin) instead of
/// revealing the hidden, wrongly textured Steve bone inside an NPC rig.
#[derive(Debug)]
pub(crate) struct FapcraftPlayerPose;
impl BodyPose for FapcraftPlayerPose {
    fn pose(&self, _age:f32, _at_rest:&[[f32;9];6], poses:&mut [[f32;9];6]) -> bool {
        // Select the active actor by entity id. Copy the state and release
        // the lock BEFORE asking social_pose() for the rendered clip; that
        // function may advance phases and locks SOCIAL itself.
        let (id,state) = {
            let Some(active)=SOCIAL.get() else {return false};
            let guard=active.lock().unwrap_or_else(|p|p.into_inner());
            let Some((&id,state))=guard.iter()
                .filter(|(_,s)|s.started.elapsed().as_secs_f32()<s.duration
                    || (s.playlist.is_some()
                        && s.session_started.elapsed().as_secs()<120))
                .filter(|(_,s)|matches!(s.action,SocialAction::Headpat
                    | SocialAction::Clip("head_pat") | SocialAction::Clip("hug"))
                    || matches!(s.action,SocialAction::Clip(name)
                        if paired_clip(name)))
                .max_by_key(|(_,s)|s.started)
            else {return false};
            (id,*state)
        };
        // This is the exact same clock and transition used by Renderer::submit
        // for Bia; the old player code used a different phase after Shift or
        // the clip boundary, so arms/legs drifted out of sync.
        let (action,time)=social_pose(id)
            .unwrap_or((state.action,state.started.elapsed().as_secs_f32()));
        let duration=match action {
            SocialAction::Clip(name)=>rigs().get(2).and_then(Option::as_ref)
                .and_then(|rig|rig.actions.get(name))
                .map_or(state.duration,|clip|clip.duration),
            _=>state.duration,
        };
        let phase=(time/duration.max(0.001)).clamp(0.0,1.0);
        let ease_in=(phase*8.0).min(1.0);
        let ease_out=((1.0-phase)*8.0).min(1.0);
        // Looping paired scenes must not fade out at the end of every clip.
        let blend=if matches!(action,SocialAction::Clip(name)
                if paired_clip(name)) {
            (state.session_started.elapsed().as_secs_f32()*6.0).min(1.0)
        } else {ease_in*ease_out};
        match action {
            SocialAction::Clip(name) if paired_clip(name) => {
                // Reuse the player's own skin and limbs, but drive them from
                // the original GeckoLib player-bone channels in the same
                // clip as the NPC, synchronized to one animation clock.
                let original = rigs().get(2).and_then(Option::as_ref)
                    .and_then(|rig| rig.actions.get(name));
                if let Some(clip)=original {
                    for (bone,part) in [("Head2",0usize),("Torso2",1),
                        ("RightArm",2),("LeftArm",3),("RightLeg",4),
                        ("LeftLeg",5)] {
                        if let Some(channels)=clip.bones.get(bone)
                            && !channels.rotation.is_empty() {
                            let degrees=sample(&channels.rotation,time,[0.0;3]);
                            poses[part][3]=degrees[0].to_radians()*blend;
                            poses[part][4]=-degrees[1].to_radians()*blend;
                            poses[part][5]=degrees[2].to_radians()*blend;
                        }
                    }
                }
            }
            SocialAction::Clip("hug") => {
                // Both forearms reach toward the character.
                poses[2][3] = -1.20 * blend;
                poses[2][4] = -0.18 * blend;
                poses[2][5] = 0.22 * blend;
                poses[3][3] = -1.20 * blend;
                poses[3][4] = 0.18 * blend;
                poses[3][5] = -0.22 * blend;
            }
            _ => {
                // The original character asset contains a separate RightArm
                // animation channel for the player's touch. Apply its real
                // timed rotations to the actual player-model right arm.
                let original = CHARACTERS.iter().position(|character| character.id == state.kind)
                    .and_then(|index| rigs().get(index))
                    .and_then(Option::as_ref)
                    .and_then(|rig| rig.headpat.as_ref())
                    .and_then(|clip| clip.bones.get("RightArm"))
                    .filter(|channels| !channels.rotation.is_empty());
                if let Some(channels) = original {
                    let degrees = sample(&channels.rotation,time,[0.0;3]);
                    poses[2][3] = degrees[0].to_radians()*blend;
                    poses[2][4] = -degrees[1].to_radians()*blend;
                    poses[2][5] = degrees[2].to_radians()*blend;
                } else {
                    let stroke = (time * std::f32::consts::TAU * 1.7).sin() * 0.13;
                    poses[2][3] = (-1.27 + stroke)*blend;
                    poses[2][5] = -0.13*blend;
                }
                poses[3][3] = -0.12*blend;
            }
        }
        true
    }
}
/// Bia's source keyframe transitions, independently testable without a
/// live world or Android. The hard clips declare 2.04167 s but repeat at
/// their `doggyReset` keyframe (0.625 s), not at the declared end.
fn bia_loop_followup(action:SocialAction,fast:bool,elapsed:f32)
    ->Option<&'static str> {
    match action {
        SocialAction::Clip("anal_slow") if fast => Some("anal_fast"),
        SocialAction::Clip("anal_fast") if !fast => Some("anal_slow"),
        SocialAction::Clip("prone_doggy_soft") if fast && elapsed>=0.375 =>
            Some("prone_doggy_hard1"),
        SocialAction::Clip("prone_doggy_hard1"|"prone_doggy_hard2"|"prone_doggy_hard3")
            if !fast => Some("prone_doggy_soft"),
        SocialAction::Clip("prone_doggy_hard1") if elapsed>=0.625 =>
            Some("prone_doggy_hard2"),
        SocialAction::Clip("prone_doggy_hard2") if elapsed>=0.625 =>
            Some("prone_doggy_hard3"),
        SocialAction::Clip("prone_doggy_hard3") if elapsed>=0.625 =>
            Some("prone_doggy_hard1"),
        _=>None,
    }
}

fn social_pose(id: u32) -> Option<(SocialAction, f32)> {
    let mut actions = SOCIAL.get()?.lock().unwrap_or_else(|p| p.into_inner());
    let state = actions.get_mut(&id)?;
    // No permanent lock if the player leaves unexpectedly.
    if state.playlist.is_some() && state.session_started.elapsed().as_secs()>120 {
        return None;
    }
    let duration = |key: &str| {
        rigs().get(2).and_then(Option::as_ref)
            .and_then(|rig| rig.actions.get(key))
            .map_or(2.0, |clip| clip.duration.clamp(0.25, 12.0))
    };
    // Source e0.java: intro -> insert -> soft; Shift controls the
    // fast loop, which has three separate hard clips. The old prototype
    // jumped between just soft and hard1 and never exposed hard2/hard3.
    // Jump requests the ending clip, not a jump of the avatar in place.
    let current=state.started.elapsed().as_secs_f32();
    let in_loop=matches!(state.action,SocialAction::Clip(
        "anal_slow"|"anal_fast"|"prone_doggy_soft"
        |"prone_doggy_hard1"|"prone_doggy_hard2"|"prone_doggy_hard3"
    ));
    if in_loop && BIA_FINISH_REQUEST.swap(false,Ordering::Relaxed) {
        let ending=if matches!(state.action,SocialAction::Clip(name)
            if name.starts_with("anal_")) {"anal_cum"}
            else {"prone_doggy_cum"};
        state.action=SocialAction::Clip(ending);
        state.duration=duration(ending);
        state.phase=state.playlist.map_or(0,|p|p.len().saturating_sub(1));
        state.started=Instant::now();
        return Some((state.action,0.0));
    }
    let fast=BIA_FAST.load(Ordering::Relaxed);
    // e0.java uses GeckoLib keyframe events, not the declared clip length:
    //   doggySwitch @ 0.375 in soft -> hard when Shift is held;
    //   doggyReset  @ 0.625 in hard -> another hard variation.
    // Every hard*.animation_length is 2.04167 s, but allowing it to
    // finish made the Rust fast mode slower than Java and Ana.
    let next_loop=bia_loop_followup(state.action,fast,current);
    if let Some(next)=next_loop {
        state.action=SocialAction::Clip(next);
        state.duration=duration(next);
        state.started=Instant::now();
    }
    if in_loop {
        let elapsed=state.started.elapsed().as_secs_f32();
        return Some((state.action,elapsed.rem_euclid(state.duration.max(0.001))));
    }
    let mut time = state.started.elapsed().as_secs_f32();
    if matches!(state.action,SocialAction::Clip("anal_wait"|"sitdownidle")) {
        return Some((state.action,time.rem_euclid(state.duration.max(0.001))));
    }
    if time >= state.duration {
        let next = state.playlist.and_then(|items| items.get(state.phase + 1)).copied();
        let Some(next) = next else {
            // Source final clip has ended. Clear both actors' session to
            // restore the mob's physics and the player's movement controls.
            if state.playlist.is_some() {
                actions.remove(&id);
            }
            return None
        };
        state.phase += 1;
        state.action = SocialAction::Clip(next);
        state.duration = duration(next);
        state.started = Instant::now();
        time = 0.0;
    }
    Some((state.action, time))
}

/// Exact categories used by Bia's decompiled e0.class
/// SoundKeyframeEvent switch. Keyframes which are transitions rather than
/// audio ('Done', 'Switch', etc.) intentionally emit no sound.
fn bia_audio_categories(effect:&str)-> &'static [(&'static str,usize)] {
    match effect {
        "sitdownMSG1" | "headpatMSG1" | "talk_hornyMSG3"
        | "talk_responseMSG2" => &[("girls.bia.breath",4)],
        "stripMSG1" | "talk_hornyMSG2" | "talk_responseMSG3"
        | "headpatMSG4" => &[("girls.bia.giggle",3)],
        "talk_hornyMSG1" => &[("girls.bia.hey",4)],
        "talk_hornyMSG4" | "talk_responseMSG1"
        | "headpatMSG3" => &[("girls.bia.huh",3)],
        "headpatMSG2" | "orgasm1" | "orgasm2" =>
            &[("girls.bia.mmm",8)],
        "anal_prepareMSG1" => &[("misc.plob",1)],
        "anal_prepareMSG2" => &[("misc.bedrustle",2)],
        "anal_startMSG1" => &[("girls.bia.mmm",8),("misc.pounding",36)],
        "anal_startMSG2" | "anal_slowMSG1" | "anal_fastMSG1" =>
            &[("misc.pounding",36),("girls.bia.ahh",8)],
        "anal_cumMSG2" => &[("girls.bia.ahh",8)],
        "slide" => &[("misc.slide",7)],
        "pound" => &[("misc.pounding",36)],
        "doggyMoan" => &[("girls.bia.ahh",8),("girls.bia.mmm",8)],
        "cum" => &[("misc.inserts",5)],
        _=>&[],
    }
}

/// e0.class specifies a few exact sample indices instead of selecting
/// randomly from the sounds. Keep these fixed for the source dialogue.
fn bia_fixed_audio_index(effect:&str,base:&str)->Option<usize> {
    match (effect,base) {
        ("anal_startMSG1","girls.bia.mmm")=>Some(3),
        ("anal_startMSG1","misc.pounding")=>Some(34),
        ("anal_prepareMSG1","misc.plob")=>Some(0),
        ("anal_prepareMSG2","misc.bedrustle")=>Some(0),
        ("headpatMSG1","girls.bia.breath")=>Some(0),
        ("headpatMSG2","girls.bia.mmm")=>Some(0),
        ("headpatMSG3","girls.bia.huh")=>Some(0),
        ("headpatMSG4","girls.bia.giggle")=>Some(1),
        ("orgasm1","girls.bia.mmm")=>Some(6),
        ("orgasm2","girls.bia.mmm")=>Some(7),
        _=>None,
    }
}

/// Exact registered sound IDs from 0.5.3's source-audited catalog.
/// Only name-matched *acoustic* keyframes are mapped for other characters;
/// keyframes such as transition Done/Ready/BlackScreen stay silent here.
static ORIGINAL_AUDIO_MAP: OnceLock<HashMap<String,HashMap<String,Vec<String>>>> = OnceLock::new();
fn original_audio_map()->&'static HashMap<String,HashMap<String,Vec<String>>> {
    ORIGINAL_AUDIO_MAP.get_or_init(||{
        let home=std::env::var_os("HORIZON_HOME").map(PathBuf::from)
            .unwrap_or_default();
        let path=home.join("horizon-mods/sexmod.hmod");
        let result=(|| -> Result<HashMap<String,HashMap<String,Vec<String>>>,String> {
            let file=File::open(path).map_err(|e|e.to_string())?;
            let mut zip=zip::ZipArchive::new(file).map_err(|e|e.to_string())?;
            let mut json=String::new();
            zip.by_name("porting/native_audio_cue_map.json")
                .map_err(|e|e.to_string())?
                .read_to_string(&mut json).map_err(|e|e.to_string())?;
            serde_json::from_str(&json).map_err(|e|e.to_string())
        })();
        result.unwrap_or_else(|e|{
            log::warn!("Fapcraft original NPC audio map not available: {e}");
            HashMap::new()
        })
    })
}

/// Queue time-aligned GeckoLib sound events once per *client* tick.
/// The same social_pose clock advances each character's animation and audio
/// cursor. Repeating clips wrap without double-playing a keyframe.
/// Returns registered sound IDs for Game's own audio engine to play.
pub(crate) fn take_bia_sound_events()->Vec<String> {
    let Some(actions)=SOCIAL.get() else {return Vec::new()};
    let active:Vec<(u32,&'static str)>={
        let guard=actions.lock().unwrap_or_else(|e|e.into_inner());
        guard.iter().map(|(&id,state)|(id,state.kind)).collect()
    };
    let mut emitted=Vec::new();
    for (id,kind) in active {
        let Some((SocialAction::Clip(name),time))=social_pose(id) else {continue};
        let Some(index)=CHARACTERS.iter().position(|character|character.id==kind)
            else {continue};
        let Some(clip)=rigs().get(index).and_then(Option::as_ref)
            .and_then(|rig|rig.actions.get(name)) else {continue};
        let mut guard=actions.lock().unwrap_or_else(|e|e.into_inner());
        let Some(state)=guard.get_mut(&id) else {continue};
        if state.sound_clip!=Some(name) {
            state.sound_clip=Some(name);
            state.sound_time=-1.0;
        }
        let previous=state.sound_time;
        state.sound_time=time;
        for cue in &clip.sound_cues {
            let crossed=if time>=previous {
                previous<cue.time && cue.time<=time
            } else {
                cue.time>previous || cue.time<=time
            };
            if !crossed {continue}
            let categories=if kind=="bia" {bia_audio_categories(&cue.effect)}
                else {&[]};
            if !categories.is_empty() {
                // Bia retains her explicit Java voice mappings and sample
                // indices. Do not replace them with a generic sound.
                let groups=if cue.effect=="doggyMoan" {
                    let choice=(id as usize + state.sound_counter as usize)%2;
                    &categories[choice..choice+1]
                } else {categories};
                for &(base,variants) in groups {
                    let index=bia_fixed_audio_index(&cue.effect,base)
                        .unwrap_or((id as usize+state.sound_counter as usize)%variants);
                    state.sound_counter=state.sound_counter.wrapping_add(1);
                    let sample=base.rsplit('.').next().unwrap_or(base);
                    emitted.push(format!("sexmod:{base}.{sample}{index}"));
                }
            } else if let Some(options)=original_audio_map().get(kind)
                .and_then(|table|table.get(&cue.effect)) {
                if !options.is_empty() {
                    let index=(id as usize + state.sound_counter as usize)
                        % options.len();
                    emitted.push(format!("sexmod:{}",options[index]));
                    state.sound_counter=state.sound_counter.wrapping_add(1);
                }
            }
        }
    }
    emitted
}

/// The active paired actor's NPC id. The original Java implementation
/// moves the actual player and camera to its separate `steve` hierarchy.
pub(crate) fn bia_active_actor() -> Option<u32> {
    let active=SOCIAL.get()?;
    let states=active.lock().unwrap_or_else(|e|e.into_inner());
    states.iter().filter(|(_,state)|state.kind=="bia"
        && state.playlist.is_some()
        && state.session_started.elapsed().as_secs()<120)
        .max_by_key(|(_,state)|state.session_started)
        .map(|(&id,_)|id)
}

/// Replicate the exact transform order in `visit` for a Bedrock bone:
/// animated translation, then Z/Y/X rotations around GeckoLib's pivot.
fn bia_bone_matrix(
    parent:glam::Mat4,bone:&Bone,clip:&Clip,time:f32
) -> glam::Mat4 {
    use glam::{Mat4,Vec3};
    let channel=clip.bones.get(&bone.name);
    let p=channel.map_or([0.0;3],|c|sample(&c.position,time,[0.0;3]));
    let rot=channel.map_or([0.0;3],|c|sample(&c.rotation,time,[0.0;3]));
    let deg=std::array::from_fn::<f32,3,_>(|i|(bone.rotation[i]+rot[i]).to_radians());
    let pivot=Vec3::new(-bone.pivot[0]/16.0,bone.pivot[1]/16.0,
        bone.pivot[2]/16.0);
    parent*Mat4::from_translation(Vec3::new(-p[0]/16.0,p[1]/16.0,p[2]/16.0))
        *Mat4::from_translation(pivot)
        *Mat4::from_rotation_z(deg[2])
        *Mat4::from_rotation_y(-deg[1])
        *Mat4::from_rotation_x(-deg[0])
        *Mat4::from_translation(-pivot)
}

/// Eye position of the Steve skeleton, relative to Bia's entity origin.
/// This is sampled from the *same clip/time* as the visible Steve and Bia
/// geometry, so the collision and POV do not remain at the old empty tile.
pub(crate) fn bia_partner_eye_offset(id:u32,body_yaw:f32)->Option<[f64;3]> {
    use glam::{Mat4,Vec3};
    let (SocialAction::Clip(key),time)=social_pose(id)? else {return None};
    if !paired_clip(key) {return None}
    let rig=rigs().get(2)?.as_ref()?;
    let clip=rig.actions.get(key)?;
    let find=|name:&str|rig.bones.iter().find(|bone|bone.name==name);
    let mut matrix=Mat4::from_rotation_y(
        geckolib_model_yaw(body_yaw).to_radians()
    );
    for name in ["steve","Torso2","Head2"] {
        matrix=bia_bone_matrix(matrix,find(name)?,clip,time);
    }
    let head=find("Head2")?;
    // Head pivot is [0,24,-16] in original geometry. The player eye is
    // about 0.11 blocks above the pivot; account for the head animation.
    let eye=matrix.transform_point3(Vec3::new(
        -head.pivot[0]/16.0,head.pivot[1]/16.0+0.11,
        head.pivot[2]/16.0));
    eye.is_finite().then_some([f64::from(eye.x),f64::from(eye.y),
        f64::from(eye.z)])
}

/// The original source animates two sets of arms. Use the GeckoLib
/// partner arm channels for the actual POV hand meshes as well, so the
/// first-person camera does not keep playing vanilla idle arm motion.
/// Third person still draws the complete Steve hierarchy.
pub(crate) fn bia_pov_hand_rotations()->Option<[[f32;3];2]> {
    if !BIA_FIRST_PERSON.load(Ordering::Relaxed) {return None}
    let id={
        let actions=SOCIAL.get()?;
        let states=actions.lock().unwrap_or_else(|e|e.into_inner());
        *states.iter().find(|(_,state)|state.kind=="bia"
            && state.playlist.is_some()
            && state.session_started.elapsed().as_secs()<120)?.0
    };
    let (SocialAction::Clip(name),time)=social_pose(id)? else {return None};
    if !paired_clip(name) {return None}
    let clip=rigs().get(2).and_then(Option::as_ref)
        .and_then(|rig|rig.actions.get(name))?;
    let angles=|bone:&str| {
        let degrees=clip.bones.get(bone)
            .map_or([0.0;3],|channels|
                sample(&channels.rotation,time,[0.0;3]));
        [degrees[0].to_radians(),degrees[1].to_radians(),
            degrees[2].to_radians()]
    };
    Some([angles("RightArm"),angles("LeftArm")])
}

/// Model variants must be rendered in separate passes with separate
/// textures: see original d_.java and cv.java. The original 'steve' subtree
/// cannot use Bia's brown 64x64 atlas, or its face/arms become mistextured.
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

/// Bedrock/GeckoLib allows tiny Molang-style numeric expressions in
/// vector components. The source contains e.g. "22.42-8", "2.7-8" and
/// "-97.0283+easeInOutSine". serde_json::Value::as_f64 returns None for
/// these strings; defaulting them to zero separated the paired rig (Ana).
/// Evaluate only finite sums/differences of literals; an optional +ease...
/// marker is animation metadata, NOT an arithmetic term or executable code.
fn bedrock_number(value:&Value, fallback:f32)->f32 {
    if let Some(n)=value.as_f64() {
        return if n.is_finite(){n as f32}else{fallback}
    }
    let Some(raw)=value.as_str() else {return fallback};
    let expr=raw.split("+ease").next().unwrap_or(raw).trim();
    if let Ok(n)=expr.parse::<f32>() {
        return if n.is_finite(){n}else{fallback}
    }
    let bytes=expr.as_bytes();
    let mut total=0.0_f32;
    let mut start=0usize;
    for i in 1..=bytes.len() {
        let boundary=i==bytes.len() || bytes[i]==b'+' || bytes[i]==b'-';
        if !boundary {continue}
        let part=expr[start..i].trim();
        let Ok(value)=part.parse::<f32>() else {return fallback};
        if !value.is_finite(){return fallback}
        total+=value;
        start=i;
    }
    if total.is_finite(){total}else{fallback}
}
fn triplet(v: Option<&Value>, default: [f32; 3]) -> [f32; 3] {
    let Some(array) = v.and_then(Value::as_array) else { return default };
    if array.len() < 3 { return default; }
    std::array::from_fn(|i|bedrock_number(&array[i],default[i]))
}
fn parse_track(value: Option<&Value>) -> Vec<Key> {
    let Some(value) = value else { return Vec::new() };
    fn key(time: f32, raw: &Value) -> Option<Key> {
        let vec = if raw.is_array() { raw } else { raw.get("vector")? };
        let easing = match raw.get("easing").and_then(Value::as_str).unwrap_or("linear") {
            "step" => Easing::Step,
            "easeInSine" => Easing::SineIn, "easeOutSine" => Easing::SineOut,
            "easeInOutSine" => Easing::SineBoth,
            "easeInQuad" => Easing::QuadIn, "easeOutQuad" => Easing::QuadOut,
            "easeInOutQuad" => Easing::QuadBoth,
            "easeInCubic" => Easing::CubicIn, "easeOutCubic" => Easing::CubicOut,
            "easeInOutCubic" => Easing::CubicBoth,
            "easeInQuart" => Easing::QuartIn, "easeOutQuart" => Easing::QuartOut,
            "easeInOutQuart" => Easing::QuartBoth,
            "easeInQuint" => Easing::QuintIn, "easeOutQuint" => Easing::QuintOut,
            "easeInOutQuint" => Easing::QuintBoth,
            "easeInBack" => Easing::BackIn, "easeOutBack" => Easing::BackOut,
            "easeInOutBack" => Easing::BackBoth,
            "easeInElastic" => Easing::ElasticIn, "easeOutElastic" => Easing::ElasticOut,
            "easeInOutElastic" => Easing::ElasticBoth,
            "easeInExpo" => Easing::ExpoIn, "easeOutExpo" => Easing::ExpoOut,
            "easeInOutExpo" => Easing::ExpoBoth,
            "easeInCirc" => Easing::CircIn, "easeOutCirc" => Easing::CircOut,
            "easeInBounce" => Easing::BounceIn,
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
    // Forge registers GeckoLib's SoundKeyframeEvent; the old port ignored
    // the entire sound_effects track, so voices were silent even when their
    // original .ogg files were installed.
    let mut sound_cues=Vec::new();
    if let Some(effects)=v.get("sound_effects").and_then(Value::as_object) {
        for (at,event) in effects {
            if let (Ok(time),Some(effect))=(at.parse::<f32>(),
                event.get("effect").and_then(Value::as_str)) {
                sound_cues.push(SoundCue{time,effect:effect.to_owned()});
            }
        }
        sound_cues.sort_by(|a,b|a.time.total_cmp(&b.time));
    }
    Some(Clip { duration: duration.max(0.001), bones, sound_cues })
}
/// GeckoLib EasingType definitions as used by the original Bedrock
/// animations. In particular, the headpat and hug clips need Back, Quart
/// and Elastic keyframes; treating those as linear distorts their timing.
fn bounce_out(x:f32)->f32 {
    let n=7.5625_f32;
    let d=2.75_f32;
    if x < 1.0/d { n*x*x }
    else if x < 2.0/d { let t=x-1.5/d; n*t*t+0.75 }
    else if x < 2.5/d { let t=x-2.25/d; n*t*t+0.9375 }
    else { let t=x-2.625/d; n*t*t+0.984375 }
}
fn ease(a: Easing, x: f32) -> f32 {
    use std::f32::consts::{PI,FRAC_PI_2,TAU};
    let t=x.clamp(0.0,1.0);
    let c1=1.70158_f32;
    let c2=c1*1.525;
    let c3=c1+1.0;
    let c4=TAU/3.0;
    let c5=TAU/4.5;
    match a {
        Easing::Linear => t,
        Easing::Step => if t>=1.0 {1.0} else {0.0},
        Easing::SineIn => 1.0 - (t*FRAC_PI_2).cos(),
        Easing::SineOut => (t*FRAC_PI_2).sin(),
        Easing::SineBoth => -(PI*t).cos().mul_add(0.5,-0.5),
        Easing::QuadIn => t*t,
        Easing::QuadOut => 1.0-(1.0-t).powi(2),
        Easing::QuadBoth => if t<0.5 {2.0*t*t} else {1.0-(-2.0*t+2.0).powi(2)*0.5},
        Easing::CubicIn => t.powi(3),
        Easing::CubicOut => 1.0-(1.0-t).powi(3),
        Easing::CubicBoth => if t<0.5 {4.0*t.powi(3)} else {1.0-(-2.0*t+2.0).powi(3)*0.5},
        Easing::QuartIn => t.powi(4),
        Easing::QuartOut => 1.0-(1.0-t).powi(4),
        Easing::QuartBoth => if t<0.5 {8.0*t.powi(4)} else {1.0-(-2.0*t+2.0).powi(4)*0.5},
        Easing::QuintIn => t.powi(5),
        Easing::QuintOut => 1.0-(1.0-t).powi(5),
        Easing::QuintBoth => if t<0.5 {16.0*t.powi(5)} else {1.0-(-2.0*t+2.0).powi(5)*0.5},
        Easing::BackIn => c3*t.powi(3)-c1*t.powi(2),
        Easing::BackOut => 1.0+c3*(t-1.0).powi(3)+c1*(t-1.0).powi(2),
        Easing::BackBoth => if t<0.5 {
            let z=2.0*t;
            z*z*((c2+1.0)*z-c2)*0.5
        } else {
            let z=2.0*t-2.0;
            (z*z*((c2+1.0)*z+c2)+2.0)*0.5
        },
        Easing::ElasticIn => if t==0.0 || t==1.0 {t}
            else { -(2.0_f32.powf(10.0*t-10.0)*( (10.0*t-10.75)*c4 ).sin()) },
        Easing::ElasticOut => if t==0.0 || t==1.0 {t}
            else {2.0_f32.powf(-10.0*t)*((10.0*t-0.75)*c4).sin()+1.0},
        Easing::ElasticBoth => if t==0.0 || t==1.0 {t}
            else if t<0.5 {
                -(2.0_f32.powf(20.0*t-10.0)*((20.0*t-11.125)*c5).sin())*0.5
            } else {
                (2.0_f32.powf(-20.0*t+10.0)*((20.0*t-11.125)*c5).sin())*0.5+1.0
            },
        Easing::ExpoIn => if t==0.0 {0.0} else {2.0_f32.powf(10.0*t-10.0)},
        Easing::ExpoOut => if t==1.0 {1.0} else {1.0-2.0_f32.powf(-10.0*t)},
        Easing::ExpoBoth => if t==0.0 || t==1.0 {t}
            else if t<0.5 {2.0_f32.powf(20.0*t-10.0)*0.5}
            else {(2.0-2.0_f32.powf(-20.0*t+10.0))*0.5},
        Easing::CircIn => 1.0-(1.0-t*t).sqrt(),
        Easing::CircOut => (1.0-(t-1.0).powi(2)).sqrt(),
        Easing::BounceIn => 1.0-bounce_out(1.0-t),
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
fn load_rig_variant(zip: &mut zip::ZipArchive<File>, character: &Character, geometry_path: &str) -> Result<Rig, String> {
    fn json(zip: &mut zip::ZipArchive<File>, name: &str) -> Result<Value, String> {
        let mut text = String::new();
        zip.by_name(name).map_err(|e| format!("{name}: {e}"))?
            .read_to_string(&mut text).map_err(|e| format!("{name}: {e}"))?;
        serde_json::from_str(&text).map_err(|e| format!("{name}: {e}"))
    }
    let geometry = json(zip, &format!("assets/sexmod/{geometry_path}"))?;
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
        // Bia's source state machine (e0.java): conversation, costume and
        // first/continuation clips. Retain each original animation key.
        "talk_horny","talk_horny2","talk_response","strip",
        "anal_prepare","anal_wait","anal_start","anal_slow","anal_fast","anal_cum",
        "prone_doggy_intro","prone_doggy_insert","prone_doggy_soft",
        "prone_doggy_hard1","prone_doggy_hard2","prone_doggy_hard3",
        "prone_doggy_cum","wave_idle",
    ] {
        if let Some(found) = clip(&anim, character.alias, suffix) {
            actions.insert(suffix.to_owned(), found);
        }
    }
    // The original Java mod has separate state machines for all eleven
    // characters. Load every real GeckoLib clip for each one (including the
    // shared namespace used by Galath/Manglelie), not only Bia's 41 clips.
    // Menu/scene controllers may only reference verified source clip names.
    if let Some(entries) = anim.get("animations").and_then(Value::as_object) {
        for key in entries.keys() {
            let suffix=key.strip_prefix(&format!("animation.{}.",character.alias))
                .or_else(||key.strip_prefix("animation.shared."));
            if let Some(suffix)=suffix {
                if !actions.contains_key(suffix) {
                    if let Some(found)=clip(&anim,character.alias,suffix)
                        .or_else(||clip(&anim,"shared",suffix)) {
                        actions.insert(suffix.to_owned(),found);
                    }
                }
            }
        }
    }
    Ok(Rig { sprite:format!("sexmod:entity/fapcraft_native/{}", character.id),bones,roots,nude:None,actions,
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

fn load_rig(zip: &mut zip::ZipArchive<File>, character: &Character) -> Result<Rig, String> {
    let mut rig = load_rig_variant(zip, character, character.geo)?;
    let wardrobe=match character.id {
        "bia"=>Some("geo/bia/bianude.geo.json"),
        "jenny"=>Some("geo/jenny/jennynude.geo.json"),
        "ellie"=>Some("geo/ellie/nude.geo.json"),
        "slime"=>Some("geo/slime/nude.geo.json"),
        _=>None,
    };
    if let Some(path)=wardrobe {
        let nude=load_rig_variant(zip,character,path)?;
        if character.id=="bia" {
            // Preserve Bia's original pillow during anal_prepare/anal_wait.
            if let Some(pillow)=nude.bones.iter().find(|b|b.name=="pillow") {
                let mut pillow=pillow.clone();
                pillow.children.clear();
                rig.roots.push(rig.bones.len());
                rig.bones.push(pillow);
            }
        }
        rig.nude=Some(Box::new(nude));
    }
    Ok(rig)
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
fn visit(rig: &Rig, index: usize, clip: Option<&Clip>, time: f32, draw:&mut dyn BlockEntityDrawContext, depth: usize, show_pillow: bool, player_pass: bool, sprite: &str) {
    if depth > 64 {return;}
    let Some(bone)=rig.bones.get(index) else {return;};
    // The Java cv/d_ renderers hide entire alternate bone subtrees. In
    // particular the Steve player-model pass requires a different skin.
    if player_pass {
        // Java cv.java chooses Steve or Alex arm geometry, never both.
        if bone.name.ends_with("Alex") || bone.name=="items" {return}
    } else if hidden_in_default_character_pose(&bone.name)
        || (bone.name=="pillow" && !show_pillow) {
        return;
    }
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
            let _=draw.quad(sprite,gecko_face(c,direction),uv,0xFFFFFF);
        }
        draw.pop();
    }
    for child in &bone.children {visit(rig,*child,clip,time,draw,depth+1,show_pillow,player_pass,sprite);}
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
        let rig = if rig.nude.is_some() && bia_undressed(entity.id) {
            rig.nude.as_deref().unwrap_or(rig)
        } else { rig };
        let speed=entity.living.map_or(0.0,|l|l.walk_animation_speed);
        let movement_clip=if speed>0.60 {rig.run.as_ref().or(rig.walk.as_ref())}
            else if speed>0.055 {rig.walk.as_ref().or(rig.run.as_ref())}
            else {rig.idle.as_ref()};
        let social_state=social_pose(entity.id);
        let show_pillow=matches!(social_state,
            Some((SocialAction::Clip(name),_)) if name.starts_with("anal_"));
        let social=social_state.and_then(|(action, seconds)|{
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
        for root in &rig.roots {
            visit(rig,*root,clip,time,draw,0,show_pillow,false,&rig.sprite);
        }
        // The vanilla Forge renderer d_.java performs a second, player-skin
        // textured traversal of its geobone 'steve'. The joint animation
        // channels are the SAME Clip and time as Bia; previous Rust releases
        // silently discarded this subtree and only posed loose player arms.
        // Hide this embedded mesh in POV so the normal first-person controls
        // and animated arms remain unobstructed. F5 may freely change view.
        if self.0==2 && social_state.is_some_and(|(action,_)|
            matches!(action,SocialAction::Clip(name) if paired_clip(name)))
            && !BIA_FIRST_PERSON.load(Ordering::Relaxed) {
            if let Some(steve)=rig.bones.iter().position(|bone|bone.name=="steve") {
                visit(rig,steve,clip,time,draw,0,false,true,
                    "sexmod:player/steve");
            }
        }
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
    fn partner_transform_keeps_original_geo_pivot() {
        let clip=Clip{duration:1.0,bones:HashMap::new(),sound_cues:vec![]};
        let bone=Bone{name:"Head2".into(),pivot:[0.0,24.0,-16.0],
            rotation:[0.0;3],children:vec![],cubes:vec![]};
        let position=bia_bone_matrix(glam::Mat4::IDENTITY,&bone,&clip,0.0)
            .transform_point3(glam::Vec3::new(0.0,1.5,-1.0));
        assert!((position-glam::Vec3::new(0.0,1.5,-1.0)).length()<1e-5);
    }
    #[test]
    fn original_bia_molang_expressions_are_not_zeroed() {
        assert!((bedrock_number(&serde_json::json!("22.42-8"),0.0)-14.42).abs()<0.0001);
        assert!((bedrock_number(&serde_json::json!("2.7-8"),0.0)+5.3).abs()<0.0001);
        assert!((bedrock_number(&serde_json::json!("-15.7+5.12"),0.0)+10.58).abs()<0.0001);
        assert!((bedrock_number(&serde_json::json!("-97.0283+easeInOutSine"),0.0)+97.0283).abs()<0.0001);
        assert_eq!(bedrock_number(&serde_json::json!("invalid"),42.0),42.0);
    }
    #[test]
    fn bia_voices_follow_original_keyframe_categories() {
        assert_eq!(bia_audio_categories("sitdownMSG1"),&[("girls.bia.breath",4)]);
        assert_eq!(bia_audio_categories("doggyMoan").len(),2);
        assert_eq!(bia_audio_categories("anal_fastMSG1").len(),2);
        assert_eq!(bia_fixed_audio_index("anal_startMSG1","girls.bia.mmm"),Some(3));
        assert!(bia_audio_categories("doggyReset").is_empty());
        let parsed=clip(&serde_json::json!({"animations":{"animation.bia.test":{
            "animation_length":2.0,"bones":{},"sound_effects":{
                "0.25":{"effect":"sitdownMSG1"},"1.2":{"effect":"pound"}
            }
        }}}),"bia","test").unwrap();
        assert_eq!(parsed.sound_cues.len(),2);
        assert_eq!(parsed.sound_cues[0].effect,"sitdownMSG1");
    }
    #[test]
    fn bia_camera_and_jump_input_are_explicit() {
        set_bia_first_person(true);
        assert!(BIA_FIRST_PERSON.load(Ordering::Relaxed));
        set_bia_first_person(false);
        assert!(!BIA_FIRST_PERSON.load(Ordering::Relaxed));
        set_bia_jump(false);
        assert!(!BIA_JUMP_HELD.load(Ordering::Relaxed));
    }
    #[test]
    fn doggy_shift_switches_at_geckolib_cues_not_animation_end() {
        assert_eq!(bia_loop_followup(SocialAction::Clip("prone_doggy_soft"),
            true,0.374),None);
        assert_eq!(bia_loop_followup(SocialAction::Clip("prone_doggy_soft"),
            true,0.375),Some("prone_doggy_hard1"));
        assert_eq!(bia_loop_followup(SocialAction::Clip("prone_doggy_hard1"),
            true,0.624),None);
        assert_eq!(bia_loop_followup(SocialAction::Clip("prone_doggy_hard1"),
            true,0.625),Some("prone_doggy_hard2"));
        assert_eq!(bia_loop_followup(SocialAction::Clip("prone_doggy_hard2"),
            true,0.625),Some("prone_doggy_hard3"));
        assert_eq!(bia_loop_followup(SocialAction::Clip("prone_doggy_hard3"),
            false,0.010),Some("prone_doggy_soft"));
        assert_eq!(bia_loop_followup(SocialAction::Clip("anal_slow"),
            true,0.001),Some("anal_fast"));
    }
    #[test]
    fn bia_keyframe_timings_match_original_assets() {
        let rig=rigs().get(2).and_then(Option::as_ref);
        if let Some(rig)=rig {
            let soft=rig.actions.get("prone_doggy_soft").unwrap();
            let hard=rig.actions.get("prone_doggy_hard1").unwrap();
            assert!(soft.sound_cues.iter().any(|cue|
                cue.effect=="doggySwitch" && (cue.time-0.375).abs()<0.001));
            assert!(hard.sound_cues.iter().any(|cue|
                cue.effect=="doggyReset" && (cue.time-0.625).abs()<0.001));
            assert!(hard.duration>0.625);
        }
    }
    #[test]
    fn bia_fast_toggle_is_explicit() {
        set_bia_fast(true);
        assert!(BIA_FAST.load(Ordering::Relaxed));
        set_bia_fast(false);
        assert!(!BIA_FAST.load(Ordering::Relaxed));
    }
    #[test]
    fn bia_original_sequences_have_mapped_start_and_end() {
        assert_eq!(BIA_DOGGY.first().copied(), Some("prone_doggy_intro"));
        assert_eq!(BIA_DOGGY.last().copied(), Some("prone_doggy_cum"));
        assert_eq!(BIA_ANA.first().copied(), Some("anal_start"));
        assert_eq!(BIA_ANA.last().copied(), Some("anal_cum"));
        assert!(!trigger_bia_sequence(17, "other"));
    }
    #[test]
    fn original_wardrobe_rigs_are_character_specific() {
        for kind in ["jenny","ellie","slime","bia"] {
            assert!(matches!(kind,"jenny"|"ellie"|"slime"|"bia"));
        }
        assert_eq!(toggle_character_dress(0,"goblin"),None);
        assert_eq!(toggle_character_dress(0,"galath"),None);
    }
    #[test]
    fn bia_outfit_switches_independently_by_entity() {
        const BIA_TEST_ENTITY: u32 = u32::MAX - 27;
        assert!(!bia_undressed(BIA_TEST_ENTITY));
        assert_eq!(toggle_bia_dress(BIA_TEST_ENTITY),Some(true));
        assert!(!bia_undressed(BIA_TEST_ENTITY));
        // The cue must finish before a second wardrobe request is accepted.
        assert_eq!(toggle_bia_dress(BIA_TEST_ENTITY),None);
    }
    #[test]
    fn social_clips_selected_without_changing_locomotion_when_absent() {
        assert_eq!(CHARACTERS.len(),11);
        assert!(social_pose(u32::MAX).is_none());
        trigger_social(u32::MAX, "jenny", "talk");
        assert_eq!(social_pose(u32::MAX).map(|x|x.0),Some(SocialAction::Talk));
        trigger_social(u32::MAX, "bia", "headpat");
        assert_eq!(social_pose(u32::MAX).map(|x|x.0),Some(SocialAction::Headpat));
        trigger_social(u32::MAX, "jenny", "wave");
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