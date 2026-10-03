//! Raw Win32 ABI used by the window backend.
//!
//! Signatures are copied from Win32 headers: user32 (window/message/input/clipboard), gdi32 (DIB blit/bitmaps),
//! dwmapi (dark title bar), shcore (DPI fallback), kernel32 (event/global memory), ole32 (COM file dialog).

use std::ffi::c_void;

pub type Handle=*mut c_void; pub type Hwnd=Handle; pub type Hdc=Handle; pub type Hinstance=Handle; pub type Hicon=Handle; pub type Hcursor=Handle; pub type Hbrush=Handle; pub type Hbitmap=Handle;
pub type Bool=i32; pub type Uint=u32; pub type Dword=u32; pub type Wparam=usize; pub type Lparam=isize; pub type Lresult=isize;

#[repr(C)] pub struct Point{pub x:i32,pub y:i32}
#[repr(C)] #[derive(Clone,Copy)] pub struct Rect{pub left:i32,pub top:i32,pub right:i32,pub bottom:i32}
#[repr(C)] pub struct Msg{pub hwnd:Hwnd,pub message:Uint,pub w_param:Wparam,pub l_param:Lparam,pub time:Dword,pub point:Point,pub private:Dword}
#[repr(C)] pub struct Paint{pub hdc:Hdc,pub erase:Bool,pub paint:Rect,pub restore:Bool,pub inc_update:Bool,pub reserved:[u8;32]}
#[repr(C)] pub struct BmiHeader{pub size:Dword,pub width:i32,pub height:i32,pub planes:u16,pub bit_count:u16,pub compression:Dword,pub size_image:Dword,pub x:i32,pub y:i32,pub used:Dword,pub important:Dword}
#[repr(C)] pub struct Bmi{pub header:BmiHeader,pub colors:[Dword;1]}
#[repr(C)] pub struct WndClass{pub size:Uint,pub style:Uint,pub proc:Option<unsafe extern "system" fn(Hwnd,Uint,Wparam,Lparam)->Lresult>,pub class_extra:i32,pub window_extra:i32,pub instance:Hinstance,pub icon:Hicon,pub cursor:Hcursor,pub background:Hbrush,pub menu:*const u16,pub name:*const u16,pub small_icon:Hicon}
#[repr(C)] pub struct MinMax{pub reserved:Point,pub max_size:Point,pub max_pos:Point,pub min_track:Point,pub max_track:Point}
#[repr(C)] pub struct HighContrast{pub size:Uint,pub flags:Dword,pub scheme:*mut u16}
#[repr(C)] pub struct IconInfo{pub icon:Bool,pub x:Dword,pub y:Dword,pub mask:Hbitmap,pub color:Hbitmap}
#[repr(C)] #[derive(Clone,Copy)] pub struct Guid{pub a:u32,pub b:u16,pub c:u16,pub d:[u8;8]}

#[link(name="user32")] unsafe extern "system"{
 pub fn RegisterClassExW(v:*const WndClass)->u16; pub fn CreateWindowExW(ex:Dword,class:*const u16,title:*const u16,style:Dword,x:i32,y:i32,w:i32,h:i32,parent:Hwnd,menu:Handle,instance:Hinstance,param:*mut c_void)->Hwnd;
 pub fn DefWindowProcW(hwnd:Hwnd,msg:Uint,w:Wparam,l:Lparam)->Lresult; pub fn DestroyWindow(hwnd:Hwnd)->Bool; pub fn ShowWindow(hwnd:Hwnd,cmd:i32)->Bool; pub fn UpdateWindow(hwnd:Hwnd)->Bool;
 pub fn PeekMessageW(msg:*mut Msg,hwnd:Hwnd,min:Uint,max:Uint,remove:Uint)->Bool; pub fn TranslateMessage(msg:*const Msg)->Bool; pub fn DispatchMessageW(msg:*const Msg)->Lresult; pub fn PostQuitMessage(code:i32);
 pub fn MsgWaitForMultipleObjects(count:Dword,handles:*const Handle,all:Bool,ms:Dword,mask:Dword)->Dword; pub fn PostMessageW(hwnd:Hwnd,msg:Uint,w:Wparam,l:Lparam)->Bool;
 pub fn BeginPaint(hwnd:Hwnd,paint:*mut Paint)->Hdc; pub fn EndPaint(hwnd:Hwnd,paint:*const Paint)->Bool; pub fn InvalidateRect(hwnd:Hwnd,rect:*const Rect,erase:Bool)->Bool;
 pub fn SetWindowLongPtrW(hwnd:Hwnd,index:i32,value:isize)->isize; pub fn GetWindowLongPtrW(hwnd:Hwnd,index:i32)->isize; pub fn SetWindowPos(hwnd:Hwnd,after:Hwnd,x:i32,y:i32,w:i32,h:i32,flags:Uint)->Bool;
 pub fn SetProcessDpiAwarenessContext(value:isize)->Bool; pub fn GetDpiForWindow(hwnd:Hwnd)->Uint; pub fn SetCapture(hwnd:Hwnd)->Hwnd; pub fn ReleaseCapture()->Bool;
 pub fn LoadCursorW(instance:Hinstance,name:*const u16)->Hcursor; pub fn SetCursor(cursor:Hcursor)->Hcursor; pub fn RegisterHotKey(hwnd:Hwnd,id:i32,mods:Uint,key:Uint)->Bool; pub fn UnregisterHotKey(hwnd:Hwnd,id:i32)->Bool;
 pub fn OpenClipboard(owner:Hwnd)->Bool; pub fn CloseClipboard()->Bool; pub fn EmptyClipboard()->Bool; pub fn SetClipboardData(format:Uint,memory:Handle)->Handle; pub fn GetClipboardData(format:Uint)->Handle; pub fn IsClipboardFormatAvailable(format:Uint)->Bool;
 pub fn SystemParametersInfoW(action:Uint,param:Uint,data:*mut c_void,flags:Uint)->Bool; pub fn CreateIconIndirect(info:*const IconInfo)->Hicon;
}
#[link(name="gdi32")] unsafe extern "system"{
 pub fn SetDIBitsToDevice(dc:Hdc,x:i32,y:i32,w:Dword,h:Dword,sx:i32,sy:i32,start:Uint,lines:Uint,bits:*const c_void,info:*const Bmi,usage:Uint)->i32;
 pub fn CreateBitmap(w:i32,h:i32,planes:Uint,bpp:Uint,bits:*const c_void)->Hbitmap; pub fn DeleteObject(object:Handle)->Bool;
}
#[link(name="dwmapi")] unsafe extern "system"{pub fn DwmSetWindowAttribute(hwnd:Hwnd,attr:Dword,value:*const c_void,size:Dword)->i32;}
#[link(name="shcore")] unsafe extern "system"{pub fn SetProcessDpiAwareness(value:i32)->i32;}
#[link(name="kernel32")] unsafe extern "system"{
 pub fn CreateEventW(attrs:*const c_void,manual:Bool,initial:Bool,name:*const u16)->Handle; pub fn SetEvent(event:Handle)->Bool; pub fn CloseHandle(handle:Handle)->Bool;
 pub fn GlobalAlloc(flags:Uint,bytes:usize)->Handle; pub fn GlobalLock(memory:Handle)->*mut c_void; pub fn GlobalUnlock(memory:Handle)->Bool; pub fn GlobalSize(memory:Handle)->usize;
}
#[link(name="ole32")] unsafe extern "system"{pub fn CoInitializeEx(r:*mut c_void,flags:Dword)->i32;pub fn CoCreateInstance(class:*const Guid,outer:*mut c_void,ctx:Dword,iid:*const Guid,out:*mut *mut c_void)->i32;pub fn CoTaskMemFree(p:*mut c_void);pub fn CoUninitialize();}
