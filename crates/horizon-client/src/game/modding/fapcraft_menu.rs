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
    phase: BedPhase,
}
#[derive(Clone, Copy, Debug)]
enum BedPhase { Navigating, Preparing(Instant), Waiting }
static BED_APPROACHES: OnceLock<Mutex<HashMap<u32, BedApproach>>> = OnceLock::new();
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
    pub(super) fn fapcraft_bia_tick(&mut self) {
        // PlayerMotion already exposes the active sneak/Shift state;
        // native_player() is not a PlayerRef and cannot be passed here.
        let fast=self.player.as_ref().is_some_and(|p| p.shift_key_down);
        super::fapcraft_native::set_bia_fast(fast);
        let waits: Vec<(u32,BedApproach)>=bed_approaches()
            .lock().unwrap_or_else(|e|e.into_inner())
            .iter().map(|(id,state)|(*id,*state)).collect();
        for (id,waiting) in waits {
            let Some(npc)=i32::try_from(id).ok()
                .and_then(|number|self.level_entity(number))
                .and_then(|target|self.target_position(target)) else {continue};
            let distance=((npc[0]-waiting.target[0]).powi(2)
                +(npc[1]-waiting.target[1]).powi(2)
                +(npc[2]-waiting.target[2]).powi(2)).sqrt();
            match waiting.phase {
                BedPhase::Navigating if distance<1.20 => {
                    let clip=if waiting.choice=="ana" {"anal_prepare"} else {"sitdown"};
                    super::fapcraft_native::trigger_social(id,"bia",clip);
                    if let Some(state)=bed_approaches()
                        .lock().unwrap_or_else(|e|e.into_inner()).get_mut(&id) {
                        state.phase=BedPhase::Preparing(Instant::now());
                    }
                }
                BedPhase::Preparing(started) if started.elapsed().as_secs_f32()
                    >= 5.52 => {
                    let clip=if waiting.choice=="ana" {"anal_wait"} else {"sitdownidle"};
                    super::fapcraft_native::trigger_social(id,"bia",clip);
                    if let Some(state)=bed_approaches()
                        .lock().unwrap_or_else(|e|e.into_inner()).get_mut(&id) {
                        state.phase=BedPhase::Waiting;
                    }
                }
                BedPhase::Waiting if distance<1.5 => {
                    if let Some(player)=self.player.as_ref().map(|p|p.position) {
                        let from_player=((npc[0]-player.x).powi(2)
                            +(npc[1]-player.y).powi(2)
                            +(npc[2]-player.z).powi(2)).sqrt();
                        if from_player<1.9 && self.fapcraft_align_player_for_contact(id as i32) {
                            bed_approaches().lock().unwrap_or_else(|e|e.into_inner())
                                .remove(&id);
                            super::fapcraft_native::trigger_bia_sequence(id,waiting.choice);
                            self.send_overlay_message(Text::literal(
                                "Bia: interacción lista; mantén Shift para cambiar el ritmo"
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }

    /// Find the closest reachable floor tile adjacent to a bed. The source
    /// mod e0.java scans Minecraft beds and refuses a blocked position.
    /// Limit our lookup to loaded chunks and require two air blocks at target.
    fn fapcraft_bia_bed_target(&self, npc_id: i32) -> Option<[f64; 3]> {
        use horizon_world::level::LevelAccess;
        let at = self.level_entity(npc_id).and_then(|entity|self.target_position(entity))?;
        let center=[at[0].floor() as i32,at[1].floor() as i32,at[2].floor() as i32];
        let mut best:Option<([f64; 3],f64)>=None;
        for dx in -13..=13 {
            for dz in -13..=13 {
                let (bx,bz)=(center[0]+dx,center[2]+dz);
                if !self.world.loaded(bx,bz) { continue; }
                for dy in -2..=2 {
                    let by=center[1]+dy;
                    let state=self.world.block(bx,by,bz);
                    let block=&*self.res.facts().get(state).name;
                    if !block.starts_with("minecraft:") || !block.ends_with("_bed") {
                        continue;
                    }
                    for (ox,oz) in [(0,1),(0,-1),(1,0),(-1,0)] {
                        let (x,z)=(bx+ox,bz+oz);
                        if !self.world.loaded(x,z) {continue}
                        if self.world.block(x,by,z).0!=0
                            || self.world.block(x,by+1,z).0!=0
                            || self.world.block(x,by-1,z).0==0 {
                            continue;
                        }
                        let goal=[x as f64+0.5,by as f64,z as f64+0.5];
                        let d=(goal[0]-at[0]).powi(2)+(goal[1]-at[1]).powi(2)
                            +(goal[2]-at[2]).powi(2);
                        if best.is_none_or(|(_,old)|d<old) {
                            best=Some((goal,d));
                        }
                    }
                }
            }
        }
        best.map(|(point,_)|point)
    }

    /// Begin the navigation; a right-click once both characters approach
    /// the bed completes the hand-off. This prevents the old instant scene.
    fn fapcraft_bia_request_bed(&mut self, id:i32, choice:&'static str) -> bool {
        let Some(target)=self.fapcraft_bia_bed_target(id) else {
            self.send_overlay_message(Text::literal(
                "Bia: necesitas una cama cercana con espacio libre"
            ));
            return false;
        };
        let Some(mob)=u32::try_from(id).ok()
            .and_then(|id|self.entity(id)).and_then(|entity|self.mob(entity)) else {
            self.send_overlay_message(Text::literal("Bia: no encuentro el NPC activo"));
            return false;
        };
        if !self.mob_move_to(mob,target,0.45) {
            self.send_overlay_message(Text::literal(
                "Bia: no puedo llegar a la cama; despeja el camino"
            ));
            return false;
        }
        if let Ok(id)=u32::try_from(id) {
            bed_approaches().lock().unwrap_or_else(|e|e.into_inner())
                .insert(id,BedApproach{choice,target,phase:BedPhase::Navigating});
        }
        self.send_overlay_message(Text::literal(
            "Bia se dirige a la cama. Acércate y vuelve a interactuar con ella."
        ));
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
                let pending=bed_approaches().lock().unwrap_or_else(|e|e.into_inner())
                    .get(&number).copied();
                if let Some(waiting)=pending {
                    let npc=self.level_entity(id).and_then(|entity|self.target_position(entity));
                    let player=self.player.as_ref().map(|p|p.position);
                    if let (Some(npc),Some(player))=(npc,player) {
                        let dist_to_bed=((npc[0]-waiting.target[0]).powi(2)
                            +(npc[2]-waiting.target[2]).powi(2)).sqrt();
                        let dist_to_player=((npc[0]-player.x).powi(2)
                            +(npc[1]-player.y).powi(2)
                            +(npc[2]-player.z).powi(2)).sqrt();
                        if dist_to_bed<1.2 && dist_to_player<2.25 {
                            if self.fapcraft_align_player_for_contact(id) {
                                bed_approaches().lock().unwrap_or_else(|e|e.into_inner()).remove(&number);
                                super::fapcraft_native::trigger_bia_sequence(number,waiting.choice);
                                self.send_overlay_message(Text::literal("Bia: interacción iniciada"));
                                return;
                            }
                        } else {
                            self.send_overlay_message(Text::literal(
                                "Bia: espera a que llegue a la cama y acércate"
                            ));
                            return;
                        }
                    }
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
