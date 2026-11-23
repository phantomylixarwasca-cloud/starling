//! Fapcraft's first real selectable interaction menu for the native Starling
//! client. The UI mirrors the vanilla Forge mod's m.java action-selector
//! workflow. Actions that require Forge server state have no fake buttons.
use super::{Game, Screen};
use horizon_client::{
    hud::{HudBatch, rgba},
    text::Text,
    ui,
};
use std::sync::{Mutex, OnceLock};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Page { Actions, Dialogue, Response }

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
fn labels(page:Page) -> &'static [(&'static str,bool)] {
    match page {
        Page::Actions=>&[
            ("Hablar...",true),
            ("Caricias",true),
            ("Saludar",true),
            ("Seguir / Esperar (pendiente)",false),
            ("Equipo / Casa (pendiente)",false),
            ("Cerrar",true),
        ],
        Page::Dialogue=>&[
            ("¿Cómo estás?",true),
            ("Cuéntame algo",true),
            ("Salúdame",true),
            ("Volver",true),
            ("Cerrar",true),
        ],
        Page::Response=>&[
            ("Volver a hablar",true),
            ("Otras acciones",true),
            ("Cerrar",true),
        ],
    }
}
fn panel(w:f32,h:f32,page:Page)->(f32,f32,f32,f32) {
    let width=(w-14.0).min(260.0).max(130.0);
    let rows=labels(page).len() as f32;
    let height=(68.0 + rows*24.0).min(h-8.0).max(100.0);
    ((w-width)*0.5,(h-height)*0.5,width,height)
}
fn button_rect(x:f32,y:f32,w:f32,i:usize)->(f32,f32,f32,f32) {
    (x+12.0,y+44.0+i as f32*24.0,w-24.0,20.0)
}

impl Game<'_> {
    /// Open a proper menu on the specific entity that was clicked. A repeated
    /// right click updates the target rather than opening an unrelated screen.
    pub(super) fn fapcraft_open_interaction(&mut self, id:i32, kind:&str) {
        let Some(kind) = (match kind {
            "jenny"=>Some("jenny"),"ellie"=>Some("ellie"),
            "slime"=>Some("slime"),"bia"=>Some("bia"),
            "bee"=>Some("bee"),"luna"=>Some("luna"),
            "allie"=>Some("allie"),"kobold"=>Some("kobold"),
            "goblin"=>Some("goblin"),"galath"=>Some("galath"),
            "manglelie"=>Some("manglelie"),_=>None,
        }) else {return};
        // This UI is compiled into Starling itself. The bundled sexmod.hmod
        // contains registries/assets, not a native ClientHost implementation,
        // so the generic native-mod open API is not always able to own it.
        // Fall back to Starling's own screen transition on the visible client.
        let opened = if self.mod_screen_open("sexmod") {
            true
        } else if self.screen == Screen::Playing
            && self.player.is_some()
            && self.renderer.is_some()
            && !self.screen_over_playing()
        {
            self.set_screen(Screen::Mod);
            if self.screen == Screen::Mod {
                self.native_mod_screen = Some("sexmod".to_owned());
                true
            } else {
                false
            }
        } else {
            false
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
        let (x,y,bw,bh)=panel(w,h,menu.page);
        batch.fill(atlas,0.0,0.0,w,h,rgba(0,0,0,145));
        batch.fill(atlas,x-2.0,y-2.0,bw+4.0,bh+4.0,rgba(185,155,100,255));
        batch.fill(atlas,x,y,bw,bh,rgba(22,27,34,245));
        let title=format!("Fapcraft · {}",name(menu.kind));
        batch.text(atlas,&title,x+12.0,y+12.0,u32::MAX,true);
        if menu.page==Page::Response {
            batch.text(atlas,menu.response,x+12.0,y+29.0,rgba(255,222,155,255),true);
        }
        for (i,&(label,enabled)) in labels(menu.page).iter().enumerate() {
            let (bx,by,ww,hh)=button_rect(x,y,bw,i);
            if by+hh>y+bh {break;}
            let hovered=enabled&&ui::hovered(self.cursor,bx,by,ww,hh);
            ui::button(batch,atlas,bx,by,ww,hh,&Text::literal(label),enabled,hovered);
        }
    }

    pub(super) fn fapcraft_interaction_click(&mut self) {
        let Some(mut menu)=*state().lock().unwrap_or_else(|e|e.into_inner()) else {return;};
        let (w,h)=self.gui_size();
        let (x,y,bw,bh)=panel(w,h,menu.page);
        let selected=labels(menu.page).iter().enumerate()
            .find(|(i,(_,enabled))|{
                let (bx,by,ww,hh)=button_rect(x,y,bw,*i);
                *enabled&&by+hh<=y+bh&&ui::hovered(self.cursor,bx,by,ww,hh)
            }).map(|(i,_)|i);
        let Some(i)=selected else {return;};
        self.audio.play_button_click();
        let mut social:Option<bool>=None;
        let mut voice=false;
        let mut close=false;
        match (menu.page,i) {
            (Page::Actions,0)=>{menu.page=Page::Dialogue;},
            (Page::Actions,1)=>{menu.response=response(menu.kind,2);menu.page=Page::Response;social=Some(true);voice=true;},
            (Page::Actions,2)=>{menu.response=response(menu.kind,0);menu.page=Page::Response;social=Some(false);voice=true;},
            (Page::Actions,5)=>close=true,
            (Page::Dialogue,0..=2)=>{
                menu.response=response(menu.kind,i);
                menu.page=Page::Response;
                social=Some(false);voice=true;
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
        if let Some(headpat)=social {
            if let Ok(entity)=u32::try_from(menu.entity){
                super::fapcraft_native::trigger_social(entity,headpat);
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
        if close {self.mod_screen_close("sexmod");}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test] fn menu_has_no_fake_functional_buttons() {
        assert_eq!(labels(Page::Actions).iter().filter(|(_,enabled)|!*enabled).count(),2);
        assert_eq!(labels(Page::Dialogue).len(),5);
        assert_eq!(name("bia"),"Bia");
        assert_eq!(response("bia",0),"Heyaaa~");
        let (_,_,w,h)=panel(480.0,320.0,Page::Actions);
        assert!(w<=480.0&&h<=320.0);
    }
}
