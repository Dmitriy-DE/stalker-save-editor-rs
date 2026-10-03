//! TextBox interaction state backed by edit::EditModel.

use crate::edit::{Clipboard,EditConfig,EditModel,Key,Modifiers,MouseSelect,Selection};
use sse_core::Result;

pub const BACKGROUND:u32=0x1A1D17;
pub const BORDER:u32=0x33382F;
pub const FOCUS:u32=0xD6A62D;
const BLINK_MS:u64=530;

pub struct TextInput{model:EditModel,focused:bool,caret_visible:bool,last_blink_ms:u64}
impl TextInput{
 pub fn new(text:&str,config:EditConfig)->Result<Self>{Ok(Self{model:EditModel::new(text,config)?,focused:false,caret_visible:false,last_blink_ms:0})}
 #[must_use] pub fn text(&self)->String{self.model.text()}
 #[must_use] pub const fn selection(&self)->Selection{self.model.selection()}
 #[must_use] pub const fn focused(&self)->bool{self.focused}
 #[must_use] pub const fn caret_visible(&self)->bool{self.focused&&self.caret_visible}
 pub fn focus(&mut self,focused:bool,now_ms:u64)->bool{let changed=self.focused!=focused;self.focused=focused;self.caret_visible=focused;self.last_blink_ms=now_ms;changed}
 pub fn tick(&mut self,now_ms:u64)->bool{if !self.focused{return false;}if now_ms.saturating_sub(self.last_blink_ms)<BLINK_MS{return false;}self.last_blink_ms=now_ms;self.caret_visible=!self.caret_visible;true}
 pub fn mouse(&mut self,grapheme:usize,clicks:u8,shift:bool){let kind=match clicks{2=>MouseSelect::Word,3..=>MouseSelect::Line,_=>MouseSelect::Caret};self.model.mouse_select(grapheme,kind,shift);self.caret_visible=true;}
 /// text is the platform WindowEvent::Key.text payload, preserving keyboard layout and composed text.
 pub fn key<C:Clipboard>(&mut self,key:Key,modifiers:Modifiers,text:Option<&str>,clipboard:&mut C)->Result<bool>{
   self.caret_visible=true;
   if !modifiers.ctrl {if let Some(value)=text {if !value.is_empty(){return self.model.insert_text(value);}}}
   self.model.key(key,modifiers,clipboard)
 }
 pub fn paste<C:Clipboard>(&mut self,c:&mut C)->Result<bool>{self.model.paste(c)}
 pub fn cut<C:Clipboard>(&mut self,c:&mut C)->Result<bool>{self.model.cut(c)}
 pub fn copy<C:Clipboard>(&self,c:&mut C)->Result<bool>{self.model.copy(c)}
 pub fn undo(&mut self)->Result<bool>{self.model.undo()}
}

#[cfg(test)] mod tests{
 use super::*; use crate::edit::{FieldMode,InputFilter};
 struct Clip(String);impl Clipboard for Clip{fn read_text(&mut self)->Result<String>{Ok(self.0.clone())}fn write_text(&mut self,t:&str)->Result<()>{self.0=t.to_owned();Ok(())}}
 #[test]fn text_payload_and_shortcuts(){let mut t=TextInput::new("",EditConfig{mode:FieldMode::SingleLine,max_graphemes:20,history_limit:20,filter:InputFilter::Any}).unwrap();let mut c=Clip(String::new());assert!(t.key(Key::Character('x'),Modifiers::default(),Some("Ж"),&mut c).unwrap());assert_eq!(t.text(),"Ж");assert!(t.key(Key::A,Modifiers{ctrl:true,shift:false},None,&mut c).unwrap());assert!(t.key(Key::C,Modifiers{ctrl:true,shift:false},None,&mut c).unwrap());assert_eq!(c.0,"Ж");}
 #[test]fn caret_blinks_only_when_focused(){let mut t=TextInput::new("",EditConfig::default()).unwrap();assert!(t.focus(true,10));assert!(!t.tick(100));assert!(t.tick(600));assert!(!t.caret_visible());}
}
