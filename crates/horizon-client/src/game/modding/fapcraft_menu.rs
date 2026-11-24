//! Fapcraft's first real selectable interaction menu for the native Starling
//! client. The UI mirrors the vanilla Forge mod's m.java action-selector
//! workflow. Actions that require Forge server state have no fake buttons.
use super::{Game, Screen};
use horizon_client::{
    hud::{HudBatch, rgba},
    text::Text,
    ui,
};
use std::{collections::HashMap, sync::{Mutex, OnceLock}, time::Instant};

/// Scene requests only become active once an actual bed is found and the NPC
/// has reached it. This intentionally does not trigger a scene by menu click.
#[derive(Clone, Copy, Debug)]
struct BedApproach {
    choice: &'static str,
    target: [f64; 3],
    yaw: f32,
    phase: BedPhase,
    started: Instant,
    retry_at: Instant,
    distance_at_retry: f64,
    stalled: u8,
    manual: bool,
}
#[derive(Clone, Copy, Debug)]
enum BedPhase { Navigating, Preparing(Instant), Waiting, Fading(Instant) }
#[derive(Clone, Copy)]
struct BedTarget { at: [f64; 3], yaw: f32 }
static BED_APPROACHES: OnceLock<Mutex<HashMap<u32, BedApproach>>> = OnceLock::new();
static BIA_SCREEN_FADE: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
#[derive(Clone,Copy)]
struct BiaPhysics { no_ai: bool, no_gravity: bool, no_physics: bool }
static BIA_PHYSICS: OnceLock<Mutex<HashMap<u32,BiaPhysics>>> = OnceLock::new();
fn bia_physics() -> &'static Mutex<HashMap<u32,BiaPhysics>> {
    BIA_PHYSICS.get_or_init(||Mutex::new(HashMap::new()))
}
fn bia_screen_fade() -> &'static Mutex<Option<Instant>> {
    BIA_SCREEN_FADE.get_or_init(|| Mutex::new(None))
}

/// The bed search chooses a clear *walking* tile beside the bed. The source
/// sitdown geometry is not centered on that tile. Stage the visible character
/// toward the bed's surface only after navigation completes.
/// e0.java's o_Vec3d is the collision-free tile next to the bed. The
/// SITDOWN and ANAL_PREPARE GeoBone keyframes already offset the visible
/// model onto the bed. Moving the whole NPC beforehand double-counts that
/// offset and leaves Bia floating/separated during her waiting clip.
fn bia_staged_pose(_choice:&str,anchor:[f64;3],_yaw:f32)->[f64;3] {
    anchor
}
/// e0.java moves Bia's entity one block toward the bed only AFTER the
/// 22-tick screen transition for PRONE_DOGGY_INTRO.
fn bia_scene_pose(choice:&str,anchor:[f64;3],yaw:f32)->[f64;3] {
    if choice!="doggy" {return anchor}
    let angle=f64::from(yaw).to_radians();
    [anchor[0]-angle.sin(),anchor[1],anchor[2]+angle.cos()]
}

fn bed_approaches() -> &'static Mutex<HashMap<u32, BedApproach>> {
    BED_APPROACHES.get_or_init(||Mutex::new(HashMap::new()))
}
/// Always return a copy outside of the mutex guard. An if-let directly on
/// lock().get().copied() keeps the guard alive throughout the if body; locking
/// it again from the click handler previously deadlocked the game thread.
fn bia_waiting_snapshot(id:u32) -> Option<BedApproach> {
    let guard=bed_approaches().lock().unwrap_or_else(|e|e.into_inner());
    guard.get(&id).copied()
}

/// The GeckoLib skeleton is offset from the entity anchor. Compare the
/// player's feet to both positions, not the camera and bone transforms.
fn bia_player_is_near(player:[f64;3],body:[f64;3],anchor:[f64;3])->bool {
    let horizontal=|at:[f64;3]| {
        let dx=player[0]-at[0];
        let dz=player[2]-at[2];
        (dx*dx+dz*dz).sqrt()
    };
    horizontal(body).min(horizontal(anchor))<=2.8
        && (player[1]-body[1]).abs().min((player[1]-anchor[1]).abs())<=2.0
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page { Actions, Dialogue, Response, Companion }

#[derive(Clone, Copy, Debug)]
struct Menu {
    entity: i32,
    kind: &'static str,
    page: Page,
    response: &'static str,
}

static OPEN: OnceLock<Mutex<Option<Menu>>> = OnceLock::new();
fn state() -> &'static Mutex<Option<Menu>> {
    OPEN.get_or_init(|| Mutex::new(None))
}
fn name(kind: &str) -> &'static str {
    match kind {
        "jenny"=>"Jenny", "ellie"=>"Ellie", "slime"=>"Slime",
        "bia"=>"Bia", "bee"=>"Bee", "luna"=>"Luna",
        "allie"=>"Allie", "kobold"=>"Kobold", "goblin"=>"Goblin",
        "galath"=>"Galath", "manglelie"=>"Manglelie",
        _=>"Personaje",
    }
}
fn response(kind:&str, choice:usize) -> &'static str {
    match (kind,choice) {
        ("jenny",0)=>"Jiji~",
        ("jenny",1)=>"Jeje~",
        ("jenny",2)=>"¿Eh?",
        ("ellie",0)=>"Mhm... Ya lo sé~",
        ("ellie",1)=>"Cuídate, cariño~",
        ("ellie",2)=>"Te protegeré, cariño~",
        ("bia",0)=>"Heyaaa~",
        ("bia",1)=>"Jiji~",
        ("bia",2)=>"Ooh, caricias!",
        ("luna",0)=>"Hola~",
        ("luna",1)=>"...",
        ("luna",2)=>"¡Hola!",
        ("allie",0)=>"¿Qué tal, mortal?",
        ("allie",1)=>"¡Dime tu deseo, mortal!",
        ("allie",2)=>"Hihihi",
        ("bee",0)=>"*Bzzzz*",
        ("bee",1)=>"*Bzz*",
        ("bee",2)=>"*Bzz!*",
        (_,0)=>"¡Hola!",
        (_,1)=>"Estoy aquí.",
        (_,2)=>"¡Qué alegría verte!",
        _=>"",
    }
}

#[derive(Clone, Copy)]
struct CharacterAction {
    label: &'static str,
    clip: &'static str,
    response: &'static str,
}

/// These entries reference real animation keys shipped in each character's
/// original GeckoLib JSON. No generic "everyone gets a headpat" fallback.
fn character_actions(kind: &str) -> &'static [CharacterAction] {
    // Every offered action was checked against the exact corresponding
    // animation namespace in the original Fapcraft 1.12.2 v1.1 JAR.
    // These are individual gestures/one-shot source clips, not fake shared
    // social actions and NOT replacements for the per-NPC Forge scene
    // state machines that will require separate porting.
    match kind {
        "jenny" => &[
            CharacterAction{label:"Saludar",clip:"wave",response:"Animación original: wave"},
            CharacterAction{label:"Alegrarse",clip:"fhappy",response:"Animación original: fhappy"},
            CharacterAction{label:"Intercambiar",clip:"payment",response:"Animación original: payment"},
            CharacterAction{label:"Bailar",clip:"fortnite",response:"Animación original: fortnite"},
            CharacterAction{label:"Cambiar de ropa",clip:"strip",response:"Animación original: strip"},
            CharacterAction{label:"Sentarse",clip:"sit",response:"Animación original: sit"},
        ],
        "ellie" => &[
            CharacterAction{label:"Abrazar",clip:"hug",response:"Animación original: hug"},
            CharacterAction{label:"Sentarse",clip:"sitdown",response:"Animación original: sitdown"},
            CharacterAction{label:"Descansar",clip:"sitdownidle",response:"Animación original: sitdownidle"},
            CharacterAction{label:"Gesto de abrazo",clip:"hugselected",response:"Animación original: hugselected"},
            CharacterAction{label:"Cambiar de ropa",clip:"strip",response:"Animación original: strip"},
            CharacterAction{label:"Impulsarse",clip:"dash",response:"Animación original: dash"},
        ],
        "slime" => &[
            CharacterAction{label:"Saltar",clip:"jumpstart",response:"Animación original: jumpstart"},
            CharacterAction{label:"Aterrizar",clip:"jumpend",response:"Animación original: jumpend"},
            CharacterAction{label:"Alegrarse",clip:"fhappy",response:"Animación original: fhappy"},
            CharacterAction{label:"Sentarse",clip:"sit",response:"Animación original: sit"},
            CharacterAction{label:"Cambiar de ropa",clip:"strip",response:"Animación original: strip"},
        ],
        "bee" => &[
            CharacterAction{label:"Gesto defensivo",clip:"attack0",response:"Animación original: attack0"},
            CharacterAction{label:"Descansar",clip:"idle",response:"Animación original: idle"},
            CharacterAction{label:"Con cofre",clip:"idle_has_chest",response:"Animación original: idle_has_chest"},
            CharacterAction{label:"Lanzar perla",clip:"throw_pearl",response:"Animación original: throw_pearl"},
        ],
        "luna" => &[
            CharacterAction{label:"Acariciar",clip:"head_pat",response:"Animación original: head_pat"},
            CharacterAction{label:"Preparar pesca",clip:"start_fishing",response:"Animación original: start_fishing"},
            CharacterAction{label:"Esperar pesca",clip:"idle_fishing",response:"Animación original: idle_fishing"},
            CharacterAction{label:"Dar pescado",clip:"throw_away",response:"Animación original: throw_away"},
            CharacterAction{label:"Sentarse",clip:"sitting_intro",response:"Animación original: sitting_intro"},
            CharacterAction{label:"Comer pescado",clip:"eat_fishing",response:"Animación original: eat_fishing"},
        ],
        "allie" => &[
            CharacterAction{label:"Invocar",clip:"summon",response:"Animación original: summon"},
            CharacterAction{label:"Aparecer",clip:"summon_normal",response:"Animación original: summon_normal"},
            CharacterAction{label:"Mostrar poderes",clip:"rich_normal",response:"Animación original: rich_normal"},
            CharacterAction{label:"Parpadear",clip:"blink",response:"Animación original: blink"},
        ],
        "kobold" => &[
            CharacterAction{label:"Dormir",clip:"sleep",response:"Animación original: sleep"},
            CharacterAction{label:"Sentarse",clip:"sit",response:"Animación original: sit"},
            CharacterAction{label:"Mochila",clip:"paymentBackpack",response:"Animación original: paymentBackpack"},
            CharacterAction{label:"Caer",clip:"fall_tree",response:"Animación original: fall_tree"},
            CharacterAction{label:"Correr",clip:"run",response:"Animación original: run"},
        ],
        "goblin" => &[
            CharacterAction{label:"Saltar",clip:"jump_1",response:"Animación original: jump_1"},
            CharacterAction{label:"Segundo salto",clip:"jump_2",response:"Animación original: jump_2"},
            CharacterAction{label:"Levantarse",clip:"stand_up",response:"Animación original: stand_up"},
            CharacterAction{label:"Esperar",clip:"await_pick_up",response:"Animación original: await_pick_up"},
            CharacterAction{label:"Sentarse",clip:"sit",response:"Animación original: sit"},
        ],
        "galath" => &[
            CharacterAction{label:"Dar moneda",clip:"give_coin",response:"Animación original: give_coin"},
            CharacterAction{label:"Invocar",clip:"summon",response:"Animación original: summon"},
            CharacterAction{label:"Impulsarse",clip:"boost",response:"Animación original: boost"},
            CharacterAction{label:"Volar",clip:"controlled_flight",response:"Animación original: controlled_flight"},
            CharacterAction{label:"Conjurar",clip:"summon_skeleton",response:"Animación original: summon_skeleton"},
            CharacterAction{label:"Levantarse",clip:"knocked_out_stand_up",response:"Animación original: knocked_out_stand_up"},
        ],
        "manglelie" => &[
            CharacterAction{label:"Enfadarse",clip:"angry_face",response:"Animación original: angry_face"},
            CharacterAction{label:"Caminar",clip:"walk",response:"Animación original: walk"},
            CharacterAction{label:"Correr",clip:"running",response:"Animación original: running"},
            CharacterAction{label:"Huir",clip:"scared_run",response:"Animación original: scared_run"},
            CharacterAction{label:"Sentarse con Galath",clip:"sit_on_galath",response:"Animación original: sit_on_galath"},
        ],
        "bia" => &[], // Dedicated fully staged Bia menu
        _ => &[],
    }
}
fn labels(kind:&str, entity:i32, page:Page) -> Vec<(&'static str,bool)> {
    if kind == "bia" {
        return match page {
            Page::Actions => vec![
                ("Acariciar",true), ("Hablar",true), ("Desnudar / Vestir",true),
            ],
            Page::Dialogue => vec![
                ("Doggy",true), ("Ana",true), ("Volver",true), ("Cerrar",true),
            ],
            Page::Response => vec![("Volver",true),("Cerrar",true)],
            Page::Companion => vec![],
        };
    }
    match page {
        Page::Actions => {
            let mut out=vec![("Hablar...",true)];
            out.extend(character_actions(kind).iter().map(|a|(a.label,true)));
            out.push(("Seguir / Casa",true));
            out.push(("Cerrar",true));
            out
        },
        Page::Companion => {
            use super::fapcraft_native::{CompanionMode, companion_order};
            let following = u32::try_from(entity).ok()
                .and_then(companion_order)
                .is_some_and(|o| o.mode==CompanionMode::Follow);
            vec![
                (if following {"Dejar de seguirme"} else {"Seguirme"},true),
                ("Ir a casa",true),
                ("Establecer casa aquí",true),
                ("Equipo (pendiente)",false),
                ("Volver",true),
                ("Cerrar",true),
            ]
        },
        Page::Dialogue=>vec![
            ("¿Cómo estás?",true),
            ("Cuéntame algo",true),
            ("Salúdame",true),
            ("Volver",true),
            ("Cerrar",true),
        ],
        Page::Response=>vec![
            ("Volver a hablar",true),
            ("Otras acciones",true),
            ("Cerrar",true),
        ],
    }
}
fn panel(w:f32,h:f32,kind:&str,entity:i32,page:Page)->(f32,f32,f32,f32) {
    if kind == "bia" {
        // Keep the NPC visible left of the screen like the original selector.
        // Still fit a narrow landscape phone without horizontal scrolling.
        let width = (w * 0.39).clamp(145.0, 270.0).min(w-10.0);
        let height = (69.0 + labels(kind,entity,page).len() as f32 * 24.0)
            .min(h-8.0);
        return ((w-width-10.0).max(2.0), ((h-height)*0.5).max(4.0), width, height);
    }
    let width=(w-14.0).min(260.0).max(130.0);
    let rows=labels(kind,entity,page).len() as f32;
    let height=(68.0 + rows*24.0).min(h-8.0).max(100.0);
    ((w-width)*0.5,(h-height)*0.5,width,height)
}
fn button_rect(x:f32,y:f32,w:f32,i:usize)->(f32,f32,f32,f32) {
    (x+12.0,y+44.0+i as f32*24.0,w-24.0,20.0)
}

// A paired scene has one authoritative physical placement. The animated
// Steve head is a *rendered bone*, not a valid collision/body transform: moving
// the player to that bone on every tick made Minecraft's collision and server
// reconciliation repeatedly teleport the camera and avatar.
static BIA_PLAYER_PLACED: OnceLock<Mutex<Option<u32>>> = OnceLock::new();
fn bia_player_placed() -> &'static Mutex<Option<u32>> {
    BIA_PLAYER_PLACED.get_or_init(||Mutex::new(None))
}
impl Game<'_> {
    /// Place the physical player only once per scene. The head of Steve
    /// animates every frame and must affect POV *rendering*, not world
    /// collisions or the network-authoritative player body.
    pub(in crate::game) fn fapcraft_bia_attach_player_to_steve(&mut self) {
        use horizon_core::geometry::Vec3;
        use horizon_world::level::LevelAccess;
        let actor=super::fapcraft_native::bia_active_actor();
        let should_place={
            let mut last=bia_player_placed().lock()
                .unwrap_or_else(|e|e.into_inner());
            if *last==actor {false}
            else {
                *last=actor;
                actor.is_some()
            }
        };
        if !should_place {return}
        let Some(id)=actor else {return};
        let Some(mob)=self.mobs.iter().find(|m|m.id==id && !m.removed)
            else {return};
        let Some(offset)=super::fapcraft_native::bia_partner_eye_offset(
            id,mob.body_yaw) else {return};
        let Some(current)=self.player.as_ref() else {return};
        let pos=Vec3::new(mob.position.x+offset[0],
            mob.position.y+offset[1]-f64::from(current.dimensions.eye_height),
            mob.position.z+offset[2]);
        if !pos.x.is_finite() || !pos.y.is_finite() || !pos.z.is_finite() {
            return
        }
        // Preserve the safe starting tile if Steve's animated bone is
        // inside a bed, the floor, or an unloaded chunk. Never teleport
        // repeatedly trying to follow the head at subsequent clip changes.
        let (x,y,z)=(pos.x.floor() as i32,pos.y.floor() as i32,
            pos.z.floor() as i32);
        if !self.world.loaded(x,z) || self.world.block(x,y,z).0!=0
            || self.world.block(x,y+1,z).0!=0
            || self.world.block(x,y-1,z).0==0 {
            return
        }
        let Some(player)=self.player.as_mut() else {return};
        player.position=pos;
        player.velocity=Vec3::ZERO;
        player.teleported=true;
        player.teleported_delta=[false;3];
        self.previous_position=pos;
    }

    /// Play only real, registered audio cues from GeckoLib's timeline.
    /// The 0.5.2 resource pack contains the original Bia and related effect
    /// sound events. Game's client audio queue owns their lifetimes.
    pub(in crate::game) fn fapcraft_bia_play_sound_events(&self) {
        if !self.native_window() {return}
        for event in super::fapcraft_native::take_bia_sound_events() {
            self.mod_sounds.queue(&mod_api::SoundPlay::ui(&event),0);
        }
    }

    /// Tick the original two-stage interaction: navigate, prepare, wait for
    /// proximity. Shift is read from the game's existing sneak-key state.
    /// A portable version of e0.java's navigation/prepare/wait/transition
    /// state machine. Retry pathing as the Forge NPC does at 60/120 ticks.
    /// A collision-checked direct step is reserved for stalled navigation,
    /// because Starling's inherited villager AI can replace the requested path.
    pub(super) fn fapcraft_bia_tick(&mut self) {
        use horizon_world::level::LevelAccess;
        use horizon_core::geometry::Vec3;
        // Shift comes from the real Android LocalPlayer in the client tick,
        // not self.player of the integrated server (which can be absent).
        let waits:Vec<(u32,BedApproach)>=bed_approaches().lock()
            .unwrap_or_else(|e|e.into_inner()).iter()
            .map(|(&id,&state)|(id,state)).collect();
        // Restore the original mob flags when an interaction ends or expires.
        // The original Forge NPC temporarily disables physics while sitting;
        // leaving this enabled forever would break normal movement after exit.
        let restoring:Vec<(u32,BiaPhysics)>=bia_physics().lock()
            .unwrap_or_else(|e|e.into_inner()).iter()
            .map(|(&id,&flags)|(id,flags)).collect();
        for (id,flags) in restoring {
            if waits.iter().any(|(active,_)|*active==id)
                || super::fapcraft_native::social_active(id) {continue}
            if let Some(mob)=self.mobs.iter_mut().find(|m|m.id==id && !m.removed) {
                mob.no_ai=flags.no_ai;
                mob.no_gravity=flags.no_gravity;
                mob.no_physics=flags.no_physics;
                mob.needs_sync=true;
            }
            bia_physics().lock().unwrap_or_else(|e|e.into_inner()).remove(&id);
        }
        for (id,waiting) in waits {
            let Some(index)=self.mobs.iter().position(|mob|mob.id==id && !mob.removed) else {
                bed_approaches().lock().unwrap_or_else(|e|e.into_inner()).remove(&id);
                continue;
            };
            let at=self.mobs[index].position;
            let dist=((at.x-waiting.target[0]).powi(2)
                +(at.y-waiting.target[1]).powi(2)
                +(at.z-waiting.target[2]).powi(2)).sqrt();
            match waiting.phase {
                BedPhase::Navigating if dist<0.65 => {
                    let mob=&mut self.mobs[index];
                    mob.nav_stop();
                    let flags=BiaPhysics{no_ai:mob.no_ai,
                        no_gravity:mob.no_gravity,no_physics:mob.no_physics};
                    bia_physics().lock().unwrap_or_else(|e|e.into_inner())
                        .entry(id).or_insert(flags);
                    // Match e0.java's sitdown: hold the animator at its source
                    // pivot, rather than adding 9/16 blocks to the entity feet.
                    mob.no_ai=true;
                    mob.no_gravity=true;
                    mob.no_physics=true;
                    let pose=bia_staged_pose(waiting.choice,
                        waiting.target,waiting.yaw);
                    mob.position=Vec3::new(pose[0],pose[1],pose[2]);
                    mob.previous=mob.position;
                    mob.velocity=Vec3::ZERO;
                    mob.yaw=waiting.yaw;
                    mob.body_yaw=waiting.yaw;
                    mob.head_yaw=waiting.yaw;
                    mob.needs_sync=true;
                    super::fapcraft_native::trigger_social(id,"bia",
                        if waiting.choice=="ana" {"anal_prepare"} else {"sitdown"});
                    if let Some(state)=bed_approaches().lock()
                        .unwrap_or_else(|e|e.into_inner()).get_mut(&id) {
                        state.phase=BedPhase::Preparing(Instant::now());
                    }
                }
                BedPhase::Navigating => {
                    if waiting.started.elapsed().as_secs()>24 {
                        bed_approaches().lock().unwrap_or_else(|e|e.into_inner())
                            .remove(&id);
                        self.send_overlay_message(Text::literal(
                            "Bia: no logré llegar a la cama; despeja el camino"
                        ));
                        continue;
                    }
                    if waiting.retry_at.elapsed().as_secs_f32()>=1.0 && !waiting.manual {
                        // A fresh route after the vanilla mob's AI goals have
                        // run prevents the old one-shot order from being lost.
                        let mut mobs=self.mobs.lift();
                        let _=mobs[index].nav_move_to(
                            &self.mob_world_base(&[]),waiting.target,0.35);
                        self.mobs.restore(mobs,
                            horizon_client::join_seq::RestoreOrder::SurvivorsFirst);
                        let stalled=if waiting.distance_at_retry-dist<0.08 {
                            waiting.stalled.saturating_add(1)
                        } else { 0 };
                        if let Some(state)=bed_approaches().lock()
                            .unwrap_or_else(|e|e.into_inner()).get_mut(&id) {
                            state.retry_at=Instant::now();
                            state.distance_at_retry=dist;
                            state.stalled=stalled;
                            state.manual=stalled>=3;
                        }
                    }
                    if waiting.manual {
                        // Last resort for an obstructed/overridden nav goal.
                        // Never phase Bia through a wall or unsupported floor.
                        let step=0.065_f64.min(dist);
                        let to=[
                            at.x+(waiting.target[0]-at.x)*step/dist.max(0.001),
                            at.y+(waiting.target[1]-at.y)*step/dist.max(0.001),
                            at.z+(waiting.target[2]-at.z)*step/dist.max(0.001),
                        ];
                        let y=to[1].floor() as i32;
                        let clear=[-0.24_f64,0.24].iter().all(|&dx| {
                            [-0.24_f64,0.24].iter().all(|&dz| {
                                let x=(to[0]+dx).floor() as i32;
                                let z=(to[2]+dz).floor() as i32;
                                self.world.loaded(x,z)
                                    && self.world.block(x,y,z).0==0
                                    && self.world.block(x,y+1,z).0==0
                                    && self.world.block(x,y-1,z).0!=0
                            })
                        });
                        if clear {
                            let mob=&mut self.mobs[index];
                            mob.position=Vec3::new(to[0],to[1],to[2]);
                            mob.yaw=waiting.yaw;
                            mob.body_yaw=waiting.yaw;
                            mob.head_yaw=waiting.yaw;
                            mob.needs_sync=true;
                        }
                    }
                }
                BedPhase::Preparing(start) if start.elapsed().as_secs_f32()>=5.52 => {
                    super::fapcraft_native::trigger_social(id,"bia",
                        if waiting.choice=="ana" {"anal_wait"} else {"sitdownidle"});
                    if let Some(state)=bed_approaches().lock()
                        .unwrap_or_else(|e|e.into_inner()).get_mut(&id) {
                        state.phase=BedPhase::Waiting;
                    }
                }
                // Check separately with fresh client and integrated Guest
                // positions; self.player may be absent on the server tick.
                BedPhase::Waiting => {}
                BedPhase::Fading(start) if start.elapsed().as_secs_f32()>=1.10 => {
                    if self.fapcraft_bia_start_scene(id,waiting) {
                        bed_approaches().lock().unwrap_or_else(|e|e.into_inner())
                            .remove(&id);
                        super::fapcraft_native::trigger_bia_sequence(id,waiting.choice);
                    } else {
                        *bia_screen_fade().lock().unwrap_or_else(|e|e.into_inner())=None;
                        if let Some(state)=bed_approaches().lock()
                            .unwrap_or_else(|e|e.into_inner()).get_mut(&id) {
                            state.phase=BedPhase::Waiting;
                        }
                        self.send_overlay_message(Text::literal(
                            "Bia: necesito espacio libre para la transición"
                        ));
                    }
                }
                _ => {}
            }
        }
        self.fapcraft_bia_check_proximity();
    }

    /// Check after server entity updates and once after the client tick.
    /// Integrated servers keep the real network player's coordinates in
    /// Guest records; reading only self.player misses that player entirely.
    pub(in crate::game) fn fapcraft_bia_check_proximity(&mut self) {
        let pending:Vec<(u32,BedApproach)>=bed_approaches().lock()
            .unwrap_or_else(|e|e.into_inner()).iter()
            .filter_map(|(&id,&state)|matches!(state.phase,BedPhase::Waiting)
                .then_some((id,state))).collect();
        if pending.is_empty() {return}
        let mut players=Vec::new();
        if let Some(player)=&self.player {
            players.push([player.position.x,player.position.y,player.position.z]);
        }
        if let Some(lan)=&self.lan {
            for guest in lan.hub.guests.simulation_iter() {
                if guest.placed && guest.participates_online()
                    && guest.dimension==self.dim.id() {
                    players.push(guest.position);
                }
            }
        }
        if players.is_empty() {return}
        for (id,waiting) in pending {
            let Some(mob)=self.mobs.iter().find(|m|m.id==id && !m.removed)
                else {continue};
            let body=[mob.position.x,mob.position.y,mob.position.z];
            if !players.iter().any(|&p|bia_player_is_near(p,body,waiting.target)) {
                continue;
            }
            let now=Instant::now();
            // Do not nest locks and do not rearm an already-fading scene.
            let armed={
                let mut states=bed_approaches().lock()
                    .unwrap_or_else(|e|e.into_inner());
                if let Some(state)=states.get_mut(&id)
                    && matches!(state.phase,BedPhase::Waiting) {
                    state.phase=BedPhase::Fading(now);
                    true
                } else {false}
            };
            if armed {
                *bia_screen_fade().lock().unwrap_or_else(|e|e.into_inner())
                    = Some(now);
            }
        }
    }

    /// The original Forge camera briefly faded to black when the player
    /// reached the waiting character. Draw the transition above the gameplay
    /// HUD so it also works in Android first-person and third-person modes.
    pub(super) fn fapcraft_bia_draw_fade(&self,batch:&mut HudBatch) {
        let mut held=bia_screen_fade().lock().unwrap_or_else(|e|e.into_inner());
        let Some(start)=*held else {return;};
        let t=start.elapsed().as_secs_f32();
        // Original em.java counts j=22 ticks (~1.1 s) before relocating
        // both actors. Hold black through that hand-off, then fade out.
        if t>=1.60 { *held=None; return; }
        let opacity=if t<0.32 {(t/0.32*255.0) as u8}
            else if t<1.18 {255}
            else {((1.60-t)/0.42*255.0) as u8};
        let (width,height)=(batch.width,batch.height);
        batch.fill(&self.gui.atlas,0.0,0.0,width,height,
            rgba(0,0,0,opacity));
    }


    /// Find the closest reachable floor tile adjacent to a bed. The source
    /// mod e0.java scans Minecraft beds and refuses a blocked position.
    /// Limit our lookup to loaded chunks and require two air blocks at target.
    fn fapcraft_bia_bed_target(&self,npc_id:i32) -> Option<BedTarget> {
        use horizon_world::level::LevelAccess;
        let at=self.level_entity(npc_id).and_then(|id|self.target_position(id))?;
        let center=[at[0].floor() as i32,at[1].floor() as i32,at[2].floor() as i32];
        // e0.java's ad[] and ai[] arrays: NPC stands next to the bed,
        // oriented toward its centre rather than occupying an arbitrary tile.
        let candidates=[
            ([0.5,0.0,-0.5],0.0),
            ([0.5,0.0,1.5],180.0),
            ([-0.5,0.0,0.5],-90.0),
            ([1.5,0.0,0.5],90.0),
        ];
        let mut best:Option<(BedTarget,f64)>=None;
        for dx in -13..=13 {
            for dz in -13..=13 {
                let (bx,bz)=(center[0]+dx,center[2]+dz);
                if !self.world.loaded(bx,bz) {continue;}
                for dy in -2..=2 {
                    let by=center[1]+dy;
                    let state=self.world.block(bx,by,bz);
                    let block=&*self.res.facts().get(state).name;
                    if !block.starts_with("minecraft:") || !block.ends_with("_bed") {
                        continue;
                    }
                    for (offset,yaw) in candidates {
                        let goal=[bx as f64+offset[0],by as f64,
                            bz as f64+offset[2]];
                        let (x,y,z)=(goal[0].floor() as i32,
                            goal[1].floor() as i32,goal[2].floor() as i32);
                        if !self.world.loaded(x,z)
                            || self.world.block(x,y,z).0!=0
                            || self.world.block(x,y+1,z).0!=0
                            || self.world.block(x,y-1,z).0==0 {continue;}
                        let dist=(goal[0]-at[0]).powi(2)
                            +(goal[1]-at[1]).powi(2)
                            +(goal[2]-at[2]).powi(2);
                        if best.is_none_or(|(_,old)|dist<old) {
                            best=Some((BedTarget{at:goal,yaw},dist));
                        }
                    }
                }
            }
        }
        best.map(|(target,_)|target)
    }


    /// Begin the navigation; a right-click once both characters approach
    /// the bed completes the hand-off. This prevents the old instant scene.
    fn fapcraft_bia_request_bed(&mut self,id:i32,choice:&'static str)->bool {
        let Some(bed)=self.fapcraft_bia_bed_target(id) else {
            self.send_overlay_message(Text::literal(
                "Bia: necesitas una cama cercana con espacio libre"
            ));
            return false;
        };
        let Some(mob_id)=u32::try_from(id).ok() else {return false};
        let Some(index)=self.mobs.iter().position(|m|m.id==mob_id && !m.removed)
        else {
            self.send_overlay_message(Text::literal(
                "Bia: la entidad móvil no está cargada"
            ));
            return false;
        };
        let mut mobs=self.mobs.lift();
        mobs[index].nav_stop();
        let started=mobs[index].nav_move_to(&self.mob_world_base(&[]),bed.at,0.35);
        self.mobs.restore(mobs,horizon_client::join_seq::RestoreOrder::SurvivorsFirst);
        if !started {
            self.send_overlay_message(Text::literal(
                "Bia: no encontré una ruta hasta la cama"
            ));
            return false;
        }
        let at=self.mobs[index].position;
        let initial=((at.x-bed.at[0]).powi(2)
            +(at.y-bed.at[1]).powi(2)
            +(at.z-bed.at[2]).powi(2)).sqrt();
        let now=Instant::now();
        bed_approaches().lock().unwrap_or_else(|e|e.into_inner()).insert(
            mob_id,BedApproach{choice,target:bed.at,yaw:bed.yaw,
                phase:BedPhase::Navigating,started:now,retry_at:now,
                distance_at_retry:initial,stalled:0,manual:false});
        self.send_overlay_message(Text::literal(
            "Bia se dirige a la cama. Acércate cuando termine de prepararse."
        ));
        true
    }

    /// Reposition both actors *under the black fade*, not when a menu option
    /// is selected. Keep player feet in a solid, non-obstructed spot.
    fn fapcraft_bia_start_scene(&mut self,id:u32,waiting:BedApproach)->bool {
        use horizon_world::level::LevelAccess;
        use horizon_core::geometry::Vec3;
        let angle=f64::from(waiting.yaw).to_radians();
        let forward=[-angle.sin(),angle.cos()];
        let lateral=[forward[1],-forward[0]];
        let base=waiting.target;
        let positions=[
            [base[0]-forward[0]*0.8,base[1],base[2]-forward[1]*0.8],
            [base[0]-forward[0]*1.3,base[1],base[2]-forward[1]*1.3],
            [base[0]+lateral[0]*1.15,base[1],base[2]+lateral[1]*1.15],
            [base[0]-lateral[0]*1.15,base[1],base[2]-lateral[1]*1.15],
        ];
        let standing=|position:[f64;3]| {
            let (x,y,z)=(position[0].floor() as i32,
                position[1].floor() as i32,position[2].floor() as i32);
            self.world.loaded(x,z) && self.world.block(x,y,z).0==0
                && self.world.block(x,y+1,z).0==0
                && self.world.block(x,y-1,z).0!=0
        };
        // Multiple adjacent beds can obstruct the first player placement.
        // Try clear neighboring tiles and, finally, the player's current
        // location instead of failing the whole scene.
        let player=positions.into_iter().find(|&pos|standing(pos))
            .or_else(||self.player.as_ref().map(|p|[p.position.x,
                p.position.y,p.position.z]).filter(|&pos|standing(pos)));
        let Some(player)=player else {return false;};
        let Some(index)=self.mobs.iter().position(|m|m.id==id && !m.removed)
            else {return false;};
        self.teleport_to(super::commands::Target::Player,player,
            Some([waiting.yaw,0.0]),false);
        // Java starts the paired scene in the player's POV. F5/Android
        // perspective toggle stays available afterwards; this is a one-time
        // initial view, not a camera lock.
        self.requested_camera_type.set(
            Some(horizon_client::camera::CameraType::FirstPerson)
        );
        let mob=&mut self.mobs[index];
        mob.nav_stop();
        mob.yaw=waiting.yaw;
        mob.body_yaw=waiting.yaw;
        mob.head_yaw=waiting.yaw;
        let pose=bia_scene_pose(waiting.choice,waiting.target,waiting.yaw);
        mob.position=Vec3::new(pose[0],pose[1],pose[2]);
        mob.previous=mob.position;
        mob.velocity=Vec3::ZERO;
        mob.needs_sync=true;
        true
    }


    /// Align the player in Bia's frontal hand-reach zone. This also keeps
    /// her face toward the player when the player initiates the gesture from
    /// the side. Refuse an action if the actor moved outside interaction reach.
    /// Collision checks prevent teleporting the player into an obstruction.
    fn fapcraft_align_player_for_contact(&mut self, entity_id:i32) -> bool {
        use horizon_world::level::LevelAccess;
        let Some(target) = self.level_entity(entity_id) else {return false};
        let Some(npc) = self.target_position(target) else {return false};
        let Some(player) = self.player.as_ref().map(|p|p.position) else {return false};
        let distance=((player.x-npc[0]).powi(2)+(player.y-npc[1]).powi(2)
            +(player.z-npc[2]).powi(2)).sqrt();
        if !distance.is_finite() || distance > 2.25 { return false; }
        let yaw = self.target_rotation(target)[0];
        let rad = f64::from(yaw).to_radians();
        let wanted = [
            npc[0] - rad.sin()*0.72,
            npc[1],
            npc[2] + rad.cos()*0.72,
        ];
        if !wanted.iter().all(|v| v.is_finite()) {return false}
        let (x,y,z)=(wanted[0].floor() as i32,wanted[1].floor() as i32,wanted[2].floor() as i32);
        if !self.world.loaded(x,z)
            || self.world.block(x,y,z).0 != 0
            || self.world.block(x,y+1,z).0 != 0
        {
            return false;
        }
        self.teleport_to(super::commands::Target::Player, wanted,
            Some([(yaw + 180.0).rem_euclid(360.0),0.0]),false);
        true
    }

    /// Open a proper menu on the specific entity that was clicked. A repeated
    /// right click updates the target rather than opening an unrelated screen.
    pub(super) fn fapcraft_open_interaction(&mut self, id:i32, kind:&str) {
        if kind=="bia" {
            if let Ok(number)=u32::try_from(id)
                && super::fapcraft_native::stop_bia_sequence(number) {
                self.send_overlay_message(Text::literal(
                    "Bia: interacción finalizada"
                ));
                return;
            }
        }
        if kind=="bia" {
            if let Ok(number)=u32::try_from(id) {
                let waiting=bia_waiting_snapshot(number);
                if let Some(waiting)=waiting {
                    if matches!(waiting.phase,BedPhase::Waiting) {
                        // Right-click proves the user can actually reach the
                        // character. It must advance the scene, not repeat a
                        // distance warning against Bia's animation pivot.
                        let now=Instant::now();
                        if let Some(state)=bed_approaches()
                            .lock().unwrap_or_else(|e|e.into_inner())
                            .get_mut(&number) {
                            state.phase=BedPhase::Fading(now);
                        }
                        *bia_screen_fade().lock()
                            .unwrap_or_else(|e|e.into_inner())=Some(now);
                        self.send_overlay_message(Text::literal(
                            "Bia: comenzando la transición"
                        ));
                    } else {
                        self.send_overlay_message(Text::literal(match waiting.phase {
                            BedPhase::Navigating => "Bia está caminando hacia la cama",
                            BedPhase::Preparing(_) => "Bia se está preparando",
                            BedPhase::Waiting => unreachable!(),
                            BedPhase::Fading(_) => "Bia: comenzando la transición",
                        }));
                    }
                    return;
                }
            }
        }


        let Some(kind) = (match kind {
            "jenny"=>Some("jenny"),"ellie"=>Some("ellie"),
            "slime"=>Some("slime"),"bia"=>Some("bia"),
            "bee"=>Some("bee"),"luna"=>Some("luna"),
            "allie"=>Some("allie"),"kobold"=>Some("kobold"),
            "goblin"=>Some("goblin"),"galath"=>Some("galath"),
            "manglelie"=>Some("manglelie"),_=>None,
        }) else {return};
        // The built-in Fapcraft interaction is part of this APK, not a
        // dynamically loaded native screen supplied by the .hmod.
        // Do not call the generic mod_screen_open host API: it may report a
        // successful request without selecting this built-in UI on Android.
        let opened = if self.screen == Screen::Playing
            && self.player.is_some()
            && self.renderer.is_some()
            && !self.screen_over_playing()
        {
            self.native_mod_screen = Some("sexmod".to_owned());
            self.set_screen(Screen::Mod);
            if self.screen == Screen::Mod {
                true
            } else {
                self.native_mod_screen = None;
                false
            }
        } else {
            self.screen == Screen::Mod && self.mod_screen() == Some("sexmod")
        };
        if opened {
            *state().lock().unwrap_or_else(|e|e.into_inner()) =
                Some(Menu{entity:id,kind,page:Page::Actions,response:""});
        } else {
            // Do not silently consume a click when the game refused to
            // change screens; tell the player what actually happened.
            log::warn!("Fapcraft interaction: could not open menu (screen={:?}, player={}, renderer={}, overlay={})",
                self.screen, self.player.is_some(), self.renderer.is_some(), self.screen_over_playing());
            self.send_overlay_message(Text::literal(
                "Fapcraft: no se pudo abrir el menu (ver registro de Starling)",
            ));
        }
    }

    pub(super) fn fapcraft_draw_interaction(&self,batch:&mut HudBatch) {
        let Some(menu)=*state().lock().unwrap_or_else(|e|e.into_inner()) else {return;};
        let atlas=&self.gui.atlas;
        let (w,h)=(batch.width,batch.height);
        let (x,y,bw,bh)=panel(w,h,menu.kind,menu.entity,menu.page);
        if menu.kind == "bia" {
            batch.fill(atlas,0.0,0.0,w,h,rgba(0,0,0,62));
            batch.fill(atlas,x-2.0,y-2.0,bw+4.0,bh+4.0,rgba(115,115,115,220));
            batch.fill(atlas,x,y,bw,bh,rgba(40,40,40,200));
            batch.text(atlas,"Bia",w*0.24,h*0.19,u32::MAX,true);
        } else {
            batch.fill(atlas,0.0,0.0,w,h,rgba(0,0,0,145));
            batch.fill(atlas,x-2.0,y-2.0,bw+4.0,bh+4.0,rgba(185,155,100,255));
            batch.fill(atlas,x,y,bw,bh,rgba(22,27,34,245));
        }
        let title=format!("Fapcraft · {}",name(menu.kind));
        batch.text(atlas,&title,x+12.0,y+12.0,u32::MAX,true);
        if menu.page==Page::Response || (menu.kind=="bia" && menu.page==Page::Dialogue) {
            batch.text(atlas,menu.response,x+12.0,y+29.0,rgba(255,222,155,255),true);
        }
        for (i,&(label,enabled)) in labels(menu.kind,menu.entity,menu.page).iter().enumerate() {
            let (bx,by,ww,hh)=button_rect(x,y,bw,i);
            if by+hh>y+bh {break;}
            let hovered=enabled&&ui::hovered(self.cursor,bx,by,ww,hh);
            ui::button(batch,atlas,bx,by,ww,hh,&Text::literal(label),enabled,hovered);
        }
    }

    pub(super) fn fapcraft_interaction_click(&mut self) {
        let Some(mut menu)=*state().lock().unwrap_or_else(|e|e.into_inner()) else {return;};
        let (w,h)=self.gui_size();
        let (x,y,bw,bh)=panel(w,h,menu.kind,menu.entity,menu.page);
        let selected=labels(menu.kind,menu.entity,menu.page).iter().enumerate()
            .find(|(i,(_,enabled))|{
                let (bx,by,ww,hh)=button_rect(x,y,bw,*i);
                *enabled&&by+hh<=y+bh&&ui::hovered(self.cursor,bx,by,ww,hh)
            }).map(|(i,_)|i);
        let Some(i)=selected else {return;};
        self.audio.play_button_click();
        let mut social:Option<&'static str>=None;
        let mut scene:Option<&'static str>=None;
        let mut voice=false;
        let mut close=false;
        match (menu.page,i) {
            (Page::Actions,0) if menu.kind=="bia" => {
                if !self.fapcraft_align_player_for_contact(menu.entity) {
                    self.send_overlay_message(Text::literal(
                        "Acércate a Bia y busca un espacio libre frente a ella",
                    ));
                    return;
                }
                social=Some("headpat");
                menu.response="¡Gracias por las caricias!";
                voice=true;
                close=true;
            },
            (Page::Actions,1) if menu.kind=="bia" => {
                menu.page=Page::Dialogue;
                menu.response="¿Te gustaría divertirnos un rato?";
                social=Some("talk_horny");
                voice=true;
            },
            (Page::Actions,2) if menu.kind=="bia" => {
                if let Ok(id)=u32::try_from(menu.entity) {
                    if super::fapcraft_native::toggle_bia_dress(id).is_some() {
                        menu.response="Bia está cambiándose";
                        social=Some("strip");
                        voice=true;
                        close=true;
                    } else {
                        self.send_overlay_message(Text::literal(
                            "Espera a que termine la animación de Bia"
                        ));
                    }
                }
            },
            (Page::Dialogue,0..=1) if menu.kind=="bia" => {
                let choice=if i==0 {"doggy"} else {"ana"};
                // Do not play a clip until there is a suitable bed and Bia
                // can start walking toward it, as the original Java requires.
                if self.fapcraft_bia_request_bed(menu.entity,choice) {
                    menu.response="Bia se dirige a la cama";
                    close=true;
                }
            },
            (Page::Dialogue,2) if menu.kind=="bia"=>menu.page=Page::Actions,
            (Page::Dialogue,3) if menu.kind=="bia"=>close=true,
            (Page::Actions,0)=>{menu.page=Page::Dialogue;},
            (Page::Actions,index) if index >= 1 && index <= character_actions(menu.kind).len() => {
                let action=character_actions(menu.kind)[index-1];
                if action.clip=="strip" {
                    let requested=u32::try_from(menu.entity).ok()
                        .and_then(|id|super::fapcraft_native::
                            toggle_character_dress(id,menu.kind));
                    if requested.is_none() {
                        self.send_overlay_message(Text::literal(
                            "Este personaje está terminando el cambio de ropa"
                        ));
                        return;
                    }
                }
                // Contact requires reach, not a remote animation while the
                // player is still several blocks from the character.
                if matches!(action.clip,"headpat"|"head_pat"|"hug") {
                    let player=self.player.as_ref().map(|p|p.position);
                    let target=self.level_entity(menu.entity)
                        .and_then(|entity|self.target_position(entity));
                    if let (Some(player),Some(target))=(player,target) {
                        let d=((player.x-target[0]).powi(2)
                            +(player.y-target[1]).powi(2)
                            +(player.z-target[2]).powi(2)).sqrt();
                        if d>2.25 {
                            self.send_overlay_message(Text::literal(
                                "Acércate al personaje para interactuar",
                            ));
                            return;
                        }
                    }
                    if !self.fapcraft_align_player_for_contact(menu.entity) {
                        self.send_overlay_message(Text::literal(
                            "Busca un espacio libre frente al personaje",
                        ));
                        return;
                    }
                }
                menu.response=action.response;
                // Forge's original m.java closes the menu when an action is
                // chosen. Return to world view so the player can see and
                // perform the paired gesture instead of staying in a dialog.
                social=Some(action.clip);
                // The original GeckoLib SoundKeyframeEvent on this very
                // character now controls audio. Playing an unrelated ambient
                // sound at click time would duplicate or mismatch the scene.
                voice=false;
                close=true;
            },
            (Page::Actions,index) if index==character_actions(menu.kind).len()+1=>{
                menu.page=Page::Companion;
            },
            (Page::Actions,index) if index==character_actions(menu.kind).len()+2=>close=true,
            (Page::Companion,0..=2)=>{
                use super::fapcraft_native::{CompanionMode,companion_order,set_companion_mode,set_companion_home};
                if let Ok(id)=u32::try_from(menu.entity) {
                    let at=self.level_entity(menu.entity).and_then(|target|self.target_position(target));
                    if let Some(at)=at {
                        match i {
                            0 => {
                                let following=companion_order(id)
                                    .is_some_and(|o|o.mode==CompanionMode::Follow);
                                set_companion_mode(id,if following {CompanionMode::Stay}
                                    else {CompanionMode::Follow},at);
                                self.send_overlay_message(Text::literal(
                                    if following {"Fapcraft: esperando aquí"} else {"Fapcraft: siguiéndote"},
                                ));
                            },
                            1 => {
                                if companion_order(id).is_some() {
                                    set_companion_mode(id,CompanionMode::ReturnHome,at);
                                    self.send_overlay_message(Text::literal("Fapcraft: volviendo a casa"));
                                } else {
                                    self.send_overlay_message(Text::literal("Primero establece una casa"));
                                }
                            },
                            2 => {
                                set_companion_home(id,at);
                                self.send_overlay_message(Text::literal("Fapcraft: casa establecida"));
                            },
                            _ => {}
                        }
                    }
                }
            },
            (Page::Companion,4)=>menu.page=Page::Actions,
            (Page::Companion,5)=>close=true,
            (Page::Dialogue,0..=2)=>{
                menu.response=response(menu.kind,i);
                social=Some("talk");voice=true;close=true;
            },
            (Page::Dialogue,3)=>menu.page=Page::Actions,
            (Page::Dialogue,4)=>close=true,
            (Page::Response,0)=>menu.page=Page::Dialogue,
            (Page::Response,1)=>menu.page=Page::Actions,
            (Page::Response,2)=>close=true,
            _=>{},
        }
        // Release lock before calling gameplay or audio APIs.
        *state().lock().unwrap_or_else(|e|e.into_inner())=(!close).then_some(menu);
        if let Some(clip)=social {
            if let Ok(entity)=u32::try_from(menu.entity){
                super::fapcraft_native::trigger_social(entity,menu.kind,clip);
                if clip=="headpat" || clip=="head_pat" || clip=="hug" {
                    self.swing_arm();
                }
            }
        }
        if let Some(choice)=scene {
            if let Ok(entity)=u32::try_from(menu.entity) {
                super::fapcraft_native::trigger_bia_sequence(entity,choice);
            }
        }
        if voice {
            let event=format!("sexmod:entity.{}.ambient",menu.kind);
            let location=self.level_entity(menu.entity)
                .and_then(|target|self.target_position(target));
            let level=self.level(self.dim.id());
            if let (Some(at),Some(level))=(location,level){
                let _=self.native_queue_sound(level,&event,at,
                    mod_api::SoundSource::Neutral,0.7,1.0,None);
            }
        }
        if close {
            if social.is_some() || scene.is_some() {
                self.send_overlay_message(Text::literal(menu.response));
            }
            self.close_mod_screen();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn ten_character_menus_are_not_generic_fallbacks() {
        for kind in ["jenny","ellie","slime","bee","luna","allie",
            "kobold","goblin","galath","manglelie"] {
            assert!(character_actions(kind).len()>=4, "{kind}");
            assert!(character_actions(kind).iter().all(|a|!a.clip.is_empty()));
        }
        assert!(character_actions("bia").is_empty());
        assert!(character_actions("other").is_empty());
        assert_ne!(character_actions("jenny")[0].clip,
            character_actions("ellie")[0].clip);
    }
    #[test] fn bia_waits_at_original_navigation_anchor() {
        let at=bia_staged_pose("doggy",[10.5,64.0,9.5],0.0);
        assert_eq!(at,[10.5,64.0,9.5]);
        let ana=bia_staged_pose("ana",[10.5,64.0,9.5],0.0);
        assert_eq!(ana,at);
    }
    #[test] fn bia_moves_one_block_only_during_doggy_transition() {
        assert_eq!(bia_scene_pose("doggy",[10.5,64.0,9.5],0.0),
            [10.5,64.0,10.5]);
        assert_eq!(bia_scene_pose("ana",[10.5,64.0,9.5],0.0),
            [10.5,64.0,9.5]);
    }
    #[test] fn right_click_snapshot_releases_mutex_before_state_change() {
        let id=927_426_163_u32;
        let now=Instant::now();
        bed_approaches().lock().unwrap().insert(id,BedApproach{
            choice:"doggy",target:[0.5,64.0,0.5],yaw:0.0,
            phase:BedPhase::Waiting,started:now,retry_at:now,
            distance_at_retry:0.0,stalled:0,manual:false,
        });
        let snapshot=bia_waiting_snapshot(id);
        assert!(matches!(snapshot.map(|s|s.phase),Some(BedPhase::Waiting)));
        // This was a deadlock when the if-let scrutinee owned the guard.
        assert!(bed_approaches().try_lock().is_ok());
        bed_approaches().lock().unwrap().remove(&id);
    }
    #[test] fn bia_proximity_uses_feet_and_body_anchor() {
        let body=[10.5,64.0,10.45];
        let anchor=[10.5,64.0,9.5];
        assert!(bia_player_is_near([10.5,64.0,10.1],body,anchor));
        assert!(bia_player_is_near([11.5,65.0,10.0],body,anchor));
        assert!(!bia_player_is_near([30.5,64.0,30.5],body,anchor));
        assert!(!bia_player_is_near([10.5,69.0,10.1],body,anchor));
    }
    #[test] fn bia_anal_prepares_beside_bed() {
        let at=bia_staged_pose("ana",[10.5,64.0,9.5],0.0);
        assert_eq!(at,[10.5,64.0,9.5]);
    }
    #[test] fn menu_has_no_fake_functional_buttons() {
        assert_eq!(labels("jenny",0,Page::Actions).len(),7);
        assert_eq!(labels("bee",0,Page::Actions).len(),4);
        assert!(character_actions("bia").is_empty());
        assert_eq!(labels("bia",0,Page::Actions).len(),3);
        assert_eq!(labels("bia",0,Page::Dialogue)[0].0,"Doggy");
        assert_eq!(labels("bia",0,Page::Dialogue)[1].0,"Ana");
        assert_eq!(character_actions("ellie")[0].clip,"hug");
        assert_eq!(labels("jenny",0,Page::Dialogue).len(),5);
        assert!(!labels("jenny",0,Page::Companion)[3].1);
        assert_eq!(name("bia"),"Bia");
        assert_eq!(response("bia",0),"Heyaaa~");
        let (_,_,w,h)=panel(480.0,320.0,"jenny",0,Page::Actions);
        assert!(w<=480.0&&h<=320.0);
    }
}
