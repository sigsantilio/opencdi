// Hardware Win32 OpenGL 1.1 presenter so Windows 10/11 enters true Exclusive Fullscreen
// and tools such as RivaTuner Statistics Server (RTSS) / PresentMon can hook SwapBuffers.
use std::ffi::{c_char, c_void};
use log::info;
use opencdi::mcd212::{SCREEN_HEIGHT, SCREEN_WIDTH};

const PFD_DRAW_TO_WINDOW: u32 = 0x0000_0004;
const PFD_SUPPORT_OPENGL: u32 = 0x0000_0020;
const PFD_DOUBLEBUFFER: u32 = 0x0000_0001;
const PFD_TYPE_RGBA: u8 = 0;
const PFD_MAIN_PLANE: u8 = 0;

const GL_COLOR_BUFFER_BIT: u32 = 0x0000_4000;
const GL_QUADS: u32 = 0x0007;
const GL_UNSIGNED_BYTE: u32 = 0x1401;
const GL_TEXTURE_2D: u32 = 0x0DE1;
const GL_BLEND: u32 = 0x0BE2;
const GL_DEPTH_TEST: u32 = 0x0B71;
const GL_NEAREST: i32 = 0x2600;
const GL_CLAMP_TO_EDGE: i32 = 0x812F;
const GL_TEXTURE_MAG_FILTER: u32 = 0x2800;
const GL_TEXTURE_MIN_FILTER: u32 = 0x2801;
const GL_TEXTURE_WRAP_S: u32 = 0x2802;
const GL_TEXTURE_WRAP_T: u32 = 0x2803;
const GL_RGBA8: i32 = 0x8058;
const GL_BGRA_EXT: u32 = 0x80E1;
const GL_UNPACK_ROW_LENGTH: u32 = 0x0CF2;

#[repr(C)]
#[allow(non_snake_case)]
struct PixelFormatDescriptor {
    nSize: u16,
    nVersion: u16,
    dwFlags: u32,
    iPixelType: u8,
    cColorBits: u8,
    cRedBits: u8,
    cRedShift: u8,
    cGreenBits: u8,
    cGreenShift: u8,
    cBlueBits: u8,
    cBlueShift: u8,
    cAlphaBits: u8,
    cAlphaShift: u8,
    cAccumBits: u8,
    cAccumRedBits: u8,
    cAccumGreenBits: u8,
    cAccumBlueBits: u8,
    cAccumAlphaBits: u8,
    cDepthBits: u8,
    cStencilBits: u8,
    cAuxBuffers: u8,
    iLayerType: u8,
    bReserved: u8,
    dwLayerMask: u32,
    dwVisibleMask: u32,
    dwDamageMask: u32,
}

#[link(name = "user32")]
unsafe extern "system" {
    fn GetDC(hwnd: *mut c_void) -> *mut c_void;
    fn ReleaseDC(hwnd: *mut c_void, hdc: *mut c_void) -> i32;
}

#[link(name = "gdi32")]
unsafe extern "system" {
    fn ChoosePixelFormat(hdc: *mut c_void, ppfd: *const PixelFormatDescriptor) -> i32;
    fn SetPixelFormat(hdc: *mut c_void, format: i32, ppfd: *const PixelFormatDescriptor) -> i32;
    fn SwapBuffers(hdc: *mut c_void) -> i32;
}

#[link(name = "opengl32")]
unsafe extern "system" {
    fn wglCreateContext(hdc: *mut c_void) -> *mut c_void;
    fn wglMakeCurrent(hdc: *mut c_void, hglrc: *mut c_void) -> i32;
    fn wglDeleteContext(hglrc: *mut c_void) -> i32;
    fn wglGetProcAddress(lpszProc: *const c_char) -> *const c_void;

    fn glGenTextures(n: i32, textures: *mut u32);
    fn glDeleteTextures(n: i32, textures: *const u32);
    fn glBindTexture(target: u32, texture: u32);
    fn glTexParameteri(target: u32, pname: u32, param: i32);
    fn glTexImage2D(target: u32, level: i32, internalformat: i32, width: i32, height: i32, border: i32, format: u32, ty: u32, pixels: *const c_void);
    fn glTexSubImage2D(target: u32, level: i32, xoffset: i32, yoffset: i32, width: i32, height: i32, format: u32, ty: u32, pixels: *const c_void);
    fn glPixelStorei(pname: u32, param: i32);
    fn glEnable(cap: u32);
    fn glDisable(cap: u32);
    fn glViewport(x: i32, y: i32, width: i32, height: i32);
    fn glClearColor(red: f32, green: f32, blue: f32, alpha: f32);
    fn glClear(mask: u32);
    fn glColor4f(red: f32, green: f32, blue: f32, alpha: f32);
    fn glBegin(mode: u32);
    fn glEnd();
    fn glTexCoord2f(s: f32, t: f32);
    fn glVertex2f(x: f32, y: f32);
}

/// Computes normalized OpenGL quad bounds `(x0, x1, y_top, y_bot)` and texture UVs `(u0, u1, v_top, v_bot)`
/// matching [`crate::BlitLayout`] cleanly, including negative `dst_x` crops in FitHeight mode.
pub(crate) fn compute_quad_ndc_uv(win_w: u32, win_h: u32, layout: crate::BlitLayout) -> Option<([f32; 4], [f32; 4])> {
    if win_w == 0 || win_h == 0 || layout.dst_w == 0 || layout.dst_h == 0 || layout.src_h == 0 {
        return None;
    }
    let ww = win_w as f32;
    let wh = win_h as f32;
    let x0 = (layout.dst_x as f32 / ww) * 2.0 - 1.0;
    let x1 = ((layout.dst_x + layout.dst_w as isize) as f32 / ww) * 2.0 - 1.0;
    let y_top = 1.0 - (layout.dst_y as f32 / wh) * 2.0;
    let y_bot = 1.0 - ((layout.dst_y + layout.dst_h as isize) as f32 / wh) * 2.0;

    let sh = SCREEN_HEIGHT as f32;
    let v_top = (layout.src_y as f32 / sh).clamp(0.0, 1.0);
    let v_bot = ((layout.src_y + layout.src_h) as f32 / sh).clamp(0.0, 1.0);

    Some(([x0, x1, y_top, y_bot], [0.0, 1.0, v_top, v_bot]))
}

pub(crate) struct WinGlPresenter {
    hwnd: *mut c_void,
    hdc: *mut c_void,
    hglrc: *mut c_void,
    tex: u32,
}

impl WinGlPresenter {
    pub fn new(hwnd: *mut c_void) -> Result<Self, String> {
        if hwnd.is_null() {
            return Err("null HWND".into());
        }
        unsafe {
            let hdc = GetDC(hwnd);
            if hdc.is_null() {
                return Err("GetDC failed".into());
            }
            let pfd = PixelFormatDescriptor {
                nSize: std::mem::size_of::<PixelFormatDescriptor>() as u16,
                nVersion: 1,
                dwFlags: PFD_DRAW_TO_WINDOW | PFD_SUPPORT_OPENGL | PFD_DOUBLEBUFFER,
                iPixelType: PFD_TYPE_RGBA,
                cColorBits: 32,
                cRedBits: 0, cRedShift: 0, cGreenBits: 0, cGreenShift: 0, cBlueBits: 0, cBlueShift: 0,
                cAlphaBits: 0, cAlphaShift: 0,
                cAccumBits: 0, cAccumRedBits: 0, cAccumGreenBits: 0, cAccumBlueBits: 0, cAccumAlphaBits: 0,
                cDepthBits: 0,
                cStencilBits: 0,
                cAuxBuffers: 0,
                iLayerType: PFD_MAIN_PLANE,
                bReserved: 0,
                dwLayerMask: 0,
                dwVisibleMask: 0,
                dwDamageMask: 0,
            };
            let pf = ChoosePixelFormat(hdc, &pfd);
            if pf == 0 || SetPixelFormat(hdc, pf, &pfd) == 0 {
                ReleaseDC(hwnd, hdc);
                return Err("ChoosePixelFormat/SetPixelFormat failed".into());
            }
            let hglrc = wglCreateContext(hdc);
            if hglrc.is_null() {
                ReleaseDC(hwnd, hdc);
                return Err("wglCreateContext failed".into());
            }
            if wglMakeCurrent(hdc, hglrc) == 0 {
                wglDeleteContext(hglrc);
                ReleaseDC(hwnd, hdc);
                return Err("wglMakeCurrent failed".into());
            }

            // OpenCDi paces frames against its own multimedia timer and audio ring; disable DWM vblank wait in WGL.
            let swap_proc = wglGetProcAddress(c"wglSwapIntervalEXT".as_ptr());
            if !swap_proc.is_null() {
                let wgl_swap_interval_ext: extern "system" fn(i32) -> i32 = std::mem::transmute(swap_proc);
                let _ = wgl_swap_interval_ext(0);
            }

            let mut tex = 0u32;
            glGenTextures(1, &mut tex);
            if tex == 0 {
                wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut());
                wglDeleteContext(hglrc);
                ReleaseDC(hwnd, hdc);
                return Err("glGenTextures failed".into());
            }
            glBindTexture(GL_TEXTURE_2D, tex);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MIN_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_MAG_FILTER, GL_NEAREST);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_S, GL_CLAMP_TO_EDGE);
            glTexParameteri(GL_TEXTURE_2D, GL_TEXTURE_WRAP_T, GL_CLAMP_TO_EDGE);
            glTexImage2D(
                GL_TEXTURE_2D,
                0,
                GL_RGBA8,
                SCREEN_WIDTH as i32,
                SCREEN_HEIGHT as i32,
                0,
                GL_BGRA_EXT,
                GL_UNSIGNED_BYTE,
                std::ptr::null(),
            );
            info!("Win32 OpenGL presenter initialised (hardware SwapBuffers + RTSS/Exclusive Fullscreen ready)");
            Ok(Self { hwnd, hdc, hglrc, tex })
        }
    }

    pub fn present(&mut self, src: &[u32], win_w: u32, win_h: u32, layout: crate::BlitLayout) -> bool {
        if win_w == 0 || win_h == 0 || src.len() < SCREEN_WIDTH * SCREEN_HEIGHT {
            return true;
        }
        let Some(([x0, x1, y_top, y_bot], [u0, u1, v_top, v_bot])) = compute_quad_ndc_uv(win_w, win_h, layout) else {
            return true;
        };
        unsafe {
            if wglMakeCurrent(self.hdc, self.hglrc) == 0 {
                return false;
            }
            glViewport(0, 0, win_w as i32, win_h as i32);
            glClearColor(0.0, 0.0, 0.0, 1.0);
            glClear(GL_COLOR_BUFFER_BIT);
            glDisable(GL_BLEND);
            glDisable(GL_DEPTH_TEST);
            glEnable(GL_TEXTURE_2D);
            glBindTexture(GL_TEXTURE_2D, self.tex);
            glPixelStorei(GL_UNPACK_ROW_LENGTH, 0);
            glTexSubImage2D(
                GL_TEXTURE_2D,
                0,
                0,
                0,
                SCREEN_WIDTH as i32,
                SCREEN_HEIGHT as i32,
                GL_BGRA_EXT,
                GL_UNSIGNED_BYTE,
                src.as_ptr() as *const c_void,
            );
            glColor4f(1.0, 1.0, 1.0, 1.0);
            glBegin(GL_QUADS);
            glTexCoord2f(u0, v_top);
            glVertex2f(x0, y_top);
            glTexCoord2f(u1, v_top);
            glVertex2f(x1, y_top);
            glTexCoord2f(u1, v_bot);
            glVertex2f(x1, y_bot);
            glTexCoord2f(u0, v_bot);
            glVertex2f(x0, y_bot);
            glEnd();
            SwapBuffers(self.hdc) != 0
        }
    }
}

impl Drop for WinGlPresenter {
    fn drop(&mut self) {
        unsafe {
            let _ = wglMakeCurrent(self.hdc, self.hglrc);
            if self.tex != 0 {
                glDeleteTextures(1, &self.tex);
            }
            let _ = wglMakeCurrent(std::ptr::null_mut(), std::ptr::null_mut());
            if !self.hglrc.is_null() {
                let _ = wglDeleteContext(self.hglrc);
            }
            if !self.hdc.is_null() && !self.hwnd.is_null() {
                let _ = ReleaseDC(self.hwnd, self.hdc);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{compute_blit_layout, ScaleMode};

    #[test]
    fn quad_ndc_uv_maps_fit_window_fit_height_and_integer_layouts() {
        let fit_win = compute_blit_layout(1920, 1080, 560, ScaleMode::FitWindow);
        let (ndc, uv) = compute_quad_ndc_uv(1920, 1080, fit_win).unwrap();
        assert!((ndc[2] - 1.0).abs() < 1e-5 && (ndc[3] + 1.0).abs() < 1e-5);
        assert!(ndc[0] > -1.0 && ndc[1] < 1.0);
        assert_eq!(uv, [0.0, 1.0, 0.0, 1.0]);

        // FitHeight in a tall narrow window has negative dst_x and crops horizontal edges outside [-1, 1].
        let fh_narrow = compute_blit_layout(800, 1080, 480, ScaleMode::FitHeight);
        let (ndc_fh, uv_fh) = compute_quad_ndc_uv(800, 1080, fh_narrow).unwrap();
        assert!(ndc_fh[0] < -1.0 && ndc_fh[1] > 1.0);
        assert!((ndc_fh[2] - 1.0).abs() < 1e-5 && (ndc_fh[3] + 1.0).abs() < 1e-5);
        assert!((uv_fh[2] - (40.0 / 560.0)).abs() < 1e-5);
        assert!((uv_fh[3] - (520.0 / 560.0)).abs() < 1e-5);

        // Zero-sized window safely returns None.
        let empty = compute_blit_layout(0, 0, 560, ScaleMode::Integer);
        assert!(compute_quad_ndc_uv(0, 0, empty).is_none());
    }
}
