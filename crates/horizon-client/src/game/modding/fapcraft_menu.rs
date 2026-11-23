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
fn bia_screen_fade() -> &'static Mutex<Option<Instant>> {
    BIA_SCREEN_FADE.get_or_init(|| Mutex::new(None))
}

fn bed_approaches() -> &'static Mutex<HashMap<u32, BedApproach>> {
    BED_APPROACHES.get_or_init(||Mutex::new(HashMap::new()))
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
    match kind {
        "jenny" => &[
            CharacterAction{label:"Saludar",clip:"wave",response:"¡Hola!"},
            CharacterAction{label:"Alegrarse",clip:"fhappy",response:"Jiji~"},
            CharacterAction{label:"Gesto de intercambio",clip:"payment",response:"¿Un trato?"},
            CharacterAction{label:"Bailar",clip:"fortnite",response:"¡Vamos!"},
        ],
        "ellie" => &[
            CharacterAction{label:"Abrazar",clip:"hug",response:"Mhm..."},
            CharacterAction{label:"Sentarse",clip:"sitdown",response:"Un descanso."},
            CharacterAction{label:"Descansar sentada",clip:"sitdownidle",response:"..."},
        ],
        // Bia's three actual Forge actions are handled by her dedicated menu.
        "bia" => &[],
        "slime" => &[
            CharacterAction{label:"Saltar",clip:"jumpstart",response:"¡Boing!"},
            CharacterAction{label:"Alegrarse",clip:"fhappy",response:"¡Bien!"},
            CharacterAction{label:"Sentarse",clip:"sit",response:"..."},
        ],
        "bee" => &[
            CharacterAction{label:"Movimiento defensivo",clip:"attack0",response:"Bzz!"},
        ],
        "luna" => &[
            CharacterAction{label:"Caricias",clip:"head_pat",response:"..."},
            CharacterAction{label:"Preparar la pesca",clip:"start_fishing",response:"¡A pescar!"},
            CharacterAction{label:"Sentarse",clip:"sitting_intro",response:"..."},
        ],
        "allie" => &[
            CharacterAction{label:"Invocar",clip:"summon",response:"Aquí estoy."},
            CharacterAction{label:"Aparecer",clip:"summon_normal",response:"Hola, mortal."},
            CharacterAction{label:"Mostrar poderes",clip:"rich_normal",response:"¡Observa!"},
        ],
        "kobold" => &[
            CharacterAction{label:"Dormir",clip:"sleep",response:"Zzz..."},
            CharacterAction{label:"Sentarse",clip:"sit",response:"..."},
            CharacterAction{label:"Parpadear",clip:"blink",response:"..."},
        ],
        "goblin" => &[
            CharacterAction{label:"Saltar",clip:"jump_1",response:"¡Hey!"},
            CharacterAction{label:"Levantarse",clip:"stand_up",response:"¡Arriba!"},
            CharacterAction{label:"Sentarse",clip:"sit",response:"..."},
        ],
        "galath" => &[
            CharacterAction{label:"Entregar moneda",clip:"give_coin",response:"Toma."},
            CharacterAction{label:"Invocar",clip:"summon",response:"..."},
            CharacterAction{label:"Impulsarse",clip:"boost",response:"..."},
        ],
        "manglelie" => &[
            CharacterAction{label:"Enfadarse",clip:"angry_face",response:"¡Eh!"},
            CharacterAction{label:"Caminar",clip:"walk",response:"..."},
            CharacterAction{label:"Correr",clip:"running",response:"..."},
        ],
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

impl Game<'_> {
    /// Tick the original two-stage interaction: navigate, prepare, wait for
    /// proximity. Shift is read from the game's existing sneak-key state.
    /// A portable version of e0.java's navigation/prepare/wait/transition
    /// state machine. Retry pathing as the Forge NPC does at 60/120 ticks.
    /// A collision-checked direct step is reserved for stalled navigation,
    /// because Starling's inherited villager AI can replace the requested path.
    pub(super) fn fapcraft_bia_tick(&mut self) {
        use horizon_world::level::LevelAccess;
        use horizon_core::geometry::Vec3;
        super::fapcraft_native::set_bia_fast(
            self.player.as_ref().is_some_and(|p| p.shift_key_down)
        );
        let waits:Vec<(u32,BedApproach)>=bed_approaches().lock()
            .unwrap_or_else(|e|e.into_inner()).iter()
            .map(|(&id,&state)|(id,state)).collect();
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
                BedPhase::Waiting if dist<1.2 => {
                    if let Some(player)=self.player.as_ref().map(|p|p.position) {
                        let near=((at.x-player.x).powi(2)
                            +(at.y-player.y).powi(2)
                            +(at.z-player.z).powi(2)).sqrt();
                        if near<=1.0 {
                            let now=Instant::now();
                            *bia_screen_fade().lock().unwrap_or_else(|e|e.into_inner())
                                = Some(now);
                            if let Some(state)=bed_approaches().lock()
                                .unwrap_or_else(|e|e.into_inner()).get_mut(&id) {
                                state.phase=BedPhase::Fading(now);
                            }
                        }
                    }
                }
                BedPhase::Fading(start) if start.elapsed().as_secs_f32()>=0.34 => {
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
    }

    /// The original Forge camera briefly faded to black when the player
    /// reached the waiting character. Draw the transition above the gameplay
    /// HUD so it also works in Android first-person and third-person modes.
    pub(super) fn fapcraft_bia_draw_fade(&self,batch:&mut HudBatch) {
        let mut held=bia_screen_fade().lock().unwrap_or_else(|e|e.into_inner());
        let Some(start)=*held else {return;};
        let t=start.elapsed().as_secs_f32();
        if t>=0.86 { *held=None; return; }
        let opacity=if t<0.32 {(t/0.32*255.0) as u8}
            else if t<0.50 {255}
            else {((0.86-t)/0.36*255.0) as u8};
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
        let player=[
            waiting.target[0]-forward[0]*0.8,
            waiting.target[1],
            waiting.target[2]-forward[1]*0.8,
        ];
        let (x,y,z)=(player[0].floor() as i32,player[1].floor() as i32,
            player[2].floor() as i32);
        if !self.world.loaded(x,z)
            || self.world.block(x,y,z).0!=0
            || self.world.block(x,y+1,z).0!=0
            || self.world.block(x,y-1,z).0==0 {return false;}
        let Some(index)=self.mobs.iter().position(|m|m.id==id && !m.removed)
            else {return false;};
        self.teleport_to(super::commands::Target::Player,player,
            Some([waiting.yaw,0.0]),false);
        let mob=&mut self.mobs[index];
        mob.nav_stop();
        mob.yaw=waiting.yaw;
        mob.body_yaw=waiting.yaw;
        mob.head_yaw=waiting.yaw;
        if waiting.choice=="doggy" {
            mob.position=Vec3::new(
                waiting.target[0]+forward[0]*0.95,
                waiting.target[1]+0.56,
                waiting.target[2]+forward[1]*0.95);
        }
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
                if let Some(waiting)=bed_approaches()
                    .lock().unwrap_or_else(|e|e.into_inner())
                    .get(&number).copied() {
                    self.send_overlay_message(Text::literal(match waiting.phase {
                        BedPhase::Navigating => "Bia está caminando hacia la cama",
                        BedPhase::Preparing(_) => "Bia se está preparando",
                        BedPhase::Waiting => "Acércate a Bia para continuar",
                        BedPhase::Fading(_) => "Bia: comenzando la transición",
                    }));
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
                voice=true;
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
