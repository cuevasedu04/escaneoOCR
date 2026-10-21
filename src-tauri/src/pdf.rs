//! Enlace mínimo con PDFium (la biblioteca PDF de Chromium) cargada desde pdfium.dll.
//! PDFium no es seguro entre hilos: todo este módulo se usa desde un solo hilo a la vez.

use libloading::Library;
use std::ffi::{c_char, c_double, c_int, c_uint, c_ulong, c_void};
use std::path::Path;

type Handle = *mut c_void;

const FPDF_PAGEOBJ_IMAGE: c_int = 3;
const FPDF_BITMAP_GRAY: c_int = 1;
const FPDF_BITMAP_BGR: c_int = 2;
const FPDF_ANNOT: c_int = 0x01;
const FPDF_ERR_PASSWORD: c_ulong = 4;

/// Máximo de objetos por página que se inspeccionan al buscar imágenes escaneadas.
const MAX_OBJECTS: c_int = 3000;

#[allow(non_snake_case)]
pub struct Pdfium {
    FPDF_GetLastError: unsafe extern "C" fn() -> c_ulong,
    FPDF_LoadMemDocument64: unsafe extern "C" fn(*const c_void, usize, *const c_char) -> Handle,
    FPDF_CloseDocument: unsafe extern "C" fn(Handle),
    FPDF_GetPageCount: unsafe extern "C" fn(Handle) -> c_int,
    FPDF_LoadPage: unsafe extern "C" fn(Handle, c_int) -> Handle,
    FPDF_ClosePage: unsafe extern "C" fn(Handle),
    FPDF_GetPageWidthF: unsafe extern "C" fn(Handle) -> f32,
    FPDF_GetPageHeightF: unsafe extern "C" fn(Handle) -> f32,
    FPDFText_LoadPage: unsafe extern "C" fn(Handle) -> Handle,
    FPDFText_ClosePage: unsafe extern "C" fn(Handle),
    FPDFText_CountChars: unsafe extern "C" fn(Handle) -> c_int,
    FPDFText_GetText: unsafe extern "C" fn(Handle, c_int, c_int, *mut u16) -> c_int,
    FPDFPage_CountObjects: unsafe extern "C" fn(Handle) -> c_int,
    FPDFPage_GetObject: unsafe extern "C" fn(Handle, c_int) -> Handle,
    FPDFPageObj_GetType: unsafe extern "C" fn(Handle) -> c_int,
    FPDFPageObj_GetBounds: unsafe extern "C" fn(Handle, *mut f32, *mut f32, *mut f32, *mut f32) -> c_int,
    FPDFBitmap_CreateEx: unsafe extern "C" fn(c_int, c_int, c_int, *mut c_void, c_int) -> Handle,
    FPDFBitmap_FillRect: unsafe extern "C" fn(Handle, c_int, c_int, c_int, c_int, c_ulong),
    FPDFBitmap_Destroy: unsafe extern "C" fn(Handle),
    FPDF_RenderPageBitmap: unsafe extern "C" fn(Handle, Handle, c_int, c_int, c_int, c_int, c_int, c_int),
    FPDFText_GetUnicode: unsafe extern "C" fn(Handle, c_int) -> c_uint,
    FPDFText_GetLooseCharBox: unsafe extern "C" fn(Handle, c_int, *mut RectF) -> c_int,
    FPDF_PageToDevice: unsafe extern "C" fn(Handle, c_int, c_int, c_int, c_int, c_int, c_double, c_double, *mut c_int, *mut c_int) -> c_int,
    FPDFImageObj_GetImagePixelSize: unsafe extern "C" fn(Handle, *mut c_uint, *mut c_uint) -> c_int,
    FPDF_CreateNewDocument: unsafe extern "C" fn() -> Handle,
    FPDF_ImportPagesByIndex: unsafe extern "C" fn(Handle, Handle, *const c_int, c_ulong, c_int) -> c_int,
    FPDFPage_New: unsafe extern "C" fn(Handle, c_int, c_double, c_double) -> Handle,
    FPDFPageObj_NewImageObj: unsafe extern "C" fn(Handle) -> Handle,
    FPDFImageObj_LoadJpegFileInline: unsafe extern "C" fn(*mut Handle, c_int, Handle, *mut FileAccess) -> c_int,
    FPDFPageObj_Transform: unsafe extern "C" fn(Handle, c_double, c_double, c_double, c_double, c_double, c_double),
    FPDFPageObj_Destroy: unsafe extern "C" fn(Handle),
    FPDFPage_InsertObject: unsafe extern "C" fn(Handle, Handle),
    FPDFPage_GenerateContent: unsafe extern "C" fn(Handle) -> c_int,
    FPDF_SaveAsCopy: unsafe extern "C" fn(Handle, *mut FileWrite, c_ulong) -> c_int,
    _lib: Library,
}

// Los punteros a función son válidos mientras viva `_lib`; el acceso se serializa fuera.
unsafe impl Send for Pdfium {}
unsafe impl Sync for Pdfium {}

macro_rules! sym {
    ($lib:expr, $name:literal) => {
        *$lib.get(concat!($name, "\0").as_bytes()).map_err(|e| format!("pdfium.dll sin {}: {e}", $name))?
    };
}

impl Pdfium {
    pub fn load(dll: &Path) -> Result<Self, String> {
        unsafe {
            let lib = Library::new(dll).map_err(|e| format!("No se pudo cargar pdfium.dll: {e}"))?;
            let init: unsafe extern "C" fn() = sym!(lib, "FPDF_InitLibrary");
            let p = Pdfium {
                FPDF_GetLastError: sym!(lib, "FPDF_GetLastError"),
                FPDF_LoadMemDocument64: sym!(lib, "FPDF_LoadMemDocument64"),
                FPDF_CloseDocument: sym!(lib, "FPDF_CloseDocument"),
                FPDF_GetPageCount: sym!(lib, "FPDF_GetPageCount"),
                FPDF_LoadPage: sym!(lib, "FPDF_LoadPage"),
                FPDF_ClosePage: sym!(lib, "FPDF_ClosePage"),
                FPDF_GetPageWidthF: sym!(lib, "FPDF_GetPageWidthF"),
                FPDF_GetPageHeightF: sym!(lib, "FPDF_GetPageHeightF"),
                FPDFText_LoadPage: sym!(lib, "FPDFText_LoadPage"),
                FPDFText_ClosePage: sym!(lib, "FPDFText_ClosePage"),
                FPDFText_CountChars: sym!(lib, "FPDFText_CountChars"),
                FPDFText_GetText: sym!(lib, "FPDFText_GetText"),
                FPDFPage_CountObjects: sym!(lib, "FPDFPage_CountObjects"),
                FPDFPage_GetObject: sym!(lib, "FPDFPage_GetObject"),
                FPDFPageObj_GetType: sym!(lib, "FPDFPageObj_GetType"),
                FPDFPageObj_GetBounds: sym!(lib, "FPDFPageObj_GetBounds"),
                FPDFBitmap_CreateEx: sym!(lib, "FPDFBitmap_CreateEx"),
                FPDFBitmap_FillRect: sym!(lib, "FPDFBitmap_FillRect"),
                FPDFBitmap_Destroy: sym!(lib, "FPDFBitmap_Destroy"),
                FPDF_RenderPageBitmap: sym!(lib, "FPDF_RenderPageBitmap"),
                FPDFText_GetUnicode: sym!(lib, "FPDFText_GetUnicode"),
                FPDFText_GetLooseCharBox: sym!(lib, "FPDFText_GetLooseCharBox"),
                FPDF_PageToDevice: sym!(lib, "FPDF_PageToDevice"),
                FPDFImageObj_GetImagePixelSize: sym!(lib, "FPDFImageObj_GetImagePixelSize"),
                FPDF_CreateNewDocument: sym!(lib, "FPDF_CreateNewDocument"),
                FPDF_ImportPagesByIndex: sym!(lib, "FPDF_ImportPagesByIndex"),
                FPDFPage_New: sym!(lib, "FPDFPage_New"),
                FPDFPageObj_NewImageObj: sym!(lib, "FPDFPageObj_NewImageObj"),
                FPDFImageObj_LoadJpegFileInline: sym!(lib, "FPDFImageObj_LoadJpegFileInline"),
                FPDFPageObj_Transform: sym!(lib, "FPDFPageObj_Transform"),
                FPDFPageObj_Destroy: sym!(lib, "FPDFPageObj_Destroy"),
                FPDFPage_InsertObject: sym!(lib, "FPDFPage_InsertObject"),
                FPDFPage_GenerateContent: sym!(lib, "FPDFPage_GenerateContent"),
                FPDF_SaveAsCopy: sym!(lib, "FPDF_SaveAsCopy"),
                _lib: lib,
            };
            init();
            Ok(p)
        }
    }

    /// Abre un PDF ya leído en memoria. `data` debe vivir tanto como el documento.
    pub fn open<'a>(&'a self, data: &'a [u8]) -> Result<Document<'a>, String> {
        let doc = unsafe { (self.FPDF_LoadMemDocument64)(data.as_ptr() as *const c_void, data.len(), std::ptr::null()) };
        if doc.is_null() {
            let code = unsafe { (self.FPDF_GetLastError)() };
            return Err(if code == FPDF_ERR_PASSWORD {
                "Protegido con contraseña".into()
            } else {
                "Archivo PDF dañado o no válido".into()
            });
        }
        Ok(Document { lib: self, doc, _data: data })
    }
}

pub struct Document<'a> {
    lib: &'a Pdfium,
    doc: Handle,
    _data: &'a [u8],
}

impl<'a> Document<'a> {
    pub fn page_count(&self) -> usize {
        unsafe { (self.lib.FPDF_GetPageCount)(self.doc) }.max(0) as usize
    }

    pub fn page(&self, index: usize) -> Result<Page<'_>, String> {
        let page = unsafe { (self.lib.FPDF_LoadPage)(self.doc, index as c_int) };
        if page.is_null() {
            return Err(format!("No se pudo leer la página {}", index + 1));
        }
        Ok(Page { lib: self.lib, page })
    }

    /// Crea un PDF nuevo con las mismas páginas: las de `replaced` se sustituyen por una
    /// imagen JPEG que cubre la página completa y el resto se copian sin tocar.
    pub fn rebuild(&self, replaced: &std::collections::BTreeMap<usize, PageImage>) -> Result<Vec<u8>, String> {
        let lib = self.lib;
        unsafe {
            let out = (lib.FPDF_CreateNewDocument)();
            if out.is_null() {
                return Err("No se pudo crear el PDF de salida".into());
            }
            let result = (|| {
                for i in 0..self.page_count() {
                    match replaced.get(&i) {
                        None => {
                            let index = i as c_int;
                            if (lib.FPDF_ImportPagesByIndex)(out, self.doc, &index, 1, index) == 0 {
                                return Err(format!("No se pudo copiar la página {}", i + 1));
                            }
                        }
                        Some(img) => {
                            let page = (lib.FPDFPage_New)(out, i as c_int, img.page_width as c_double, img.page_height as c_double);
                            if page.is_null() {
                                return Err(format!("No se pudo crear la página {}", i + 1));
                            }
                            let obj = (lib.FPDFPageObj_NewImageObj)(out);
                            let mut access = FileAccess::new(&img.jpeg);
                            let mut pages = [page];
                            if obj.is_null() || (lib.FPDFImageObj_LoadJpegFileInline)(pages.as_mut_ptr(), 1, obj, &mut access) == 0 {
                                if !obj.is_null() {
                                    (lib.FPDFPageObj_Destroy)(obj);
                                }
                                (lib.FPDF_ClosePage)(page);
                                return Err(format!("No se pudo insertar la imagen de la página {}", i + 1));
                            }
                            (lib.FPDFPageObj_Transform)(obj, img.page_width as c_double, 0.0, 0.0, img.page_height as c_double, 0.0, 0.0);
                            (lib.FPDFPage_InsertObject)(page, obj);
                            let ok = (lib.FPDFPage_GenerateContent)(page) != 0;
                            (lib.FPDF_ClosePage)(page);
                            if !ok {
                                return Err(format!("No se pudo generar la página {}", i + 1));
                            }
                        }
                    }
                }
                let mut writer = Writer::new();
                if (lib.FPDF_SaveAsCopy)(out, &mut writer.header, FPDF_NO_INCREMENTAL) == 0 {
                    return Err("No se pudo guardar el PDF de salida".into());
                }
                Ok(std::mem::take(&mut writer.data))
            })();
            (lib.FPDF_CloseDocument)(out);
            result
        }
    }
}

impl Drop for Document<'_> {
    fn drop(&mut self) {
        unsafe { (self.lib.FPDF_CloseDocument)(self.doc) }
    }
}

const FPDF_NO_INCREMENTAL: c_ulong = 2;

/// Imagen ya codificada que reemplaza a una página completa.
pub struct PageImage {
    pub jpeg: Vec<u8>,
    pub page_width: f32,
    pub page_height: f32,
}

#[repr(C)]
pub struct RectF {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

/// FPDF_FILEACCESS: PDFium lee el JPEG por bloques desde memoria.
#[repr(C)]
pub struct FileAccess {
    len: c_ulong,
    get_block: unsafe extern "C" fn(*mut c_void, c_ulong, *mut u8, c_ulong) -> c_int,
    param: *mut c_void,
}

/// Datos que lee `FileAccess`; viven en el montón mientras exista el FileAccess.
struct Source {
    ptr: *const u8,
    len: usize,
}

impl FileAccess {
    fn new(data: &[u8]) -> Self {
        unsafe extern "C" fn get_block(param: *mut c_void, pos: c_ulong, buf: *mut u8, size: c_ulong) -> c_int {
            let src = &*(param as *const Source);
            let (pos, size) = (pos as usize, size as usize);
            if pos + size > src.len {
                return 0;
            }
            std::ptr::copy_nonoverlapping(src.ptr.add(pos), buf, size);
            1
        }
        let src = Box::new(Source { ptr: data.as_ptr(), len: data.len() });
        FileAccess { len: data.len() as c_ulong, get_block, param: Box::into_raw(src) as *mut c_void }
    }
}

impl Drop for FileAccess {
    fn drop(&mut self) {
        unsafe { drop(Box::from_raw(self.param as *mut Source)) }
    }
}

/// FPDF_FILEWRITE: PDFium entrega el PDF guardado por bloques.
#[repr(C)]
pub struct FileWrite {
    version: c_int,
    write_block: unsafe extern "C" fn(*mut FileWrite, *const c_void, c_ulong) -> c_int,
}

/// `header` va primero (repr C) para poder recuperar el Writer desde el puntero que recibe PDFium.
#[repr(C)]
struct Writer {
    header: FileWrite,
    data: Vec<u8>,
}

impl Writer {
    fn new() -> Box<Writer> {
        unsafe extern "C" fn write_block(this: *mut FileWrite, data: *const c_void, size: c_ulong) -> c_int {
            let writer = &mut *(this as *mut Writer);
            writer.data.extend_from_slice(std::slice::from_raw_parts(data as *const u8, size as usize));
            1
        }
        Box::new(Writer { header: FileWrite { version: 1, write_block }, data: Vec::new() })
    }
}

pub struct Page<'a> {
    lib: &'a Pdfium,
    page: Handle,
}

pub struct Gray {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
}

/// Imagen BGR de 3 bytes por píxel, sin relleno entre filas.
pub struct Color {
    pub width: i32,
    pub height: i32,
    pub pixels: Vec<u8>,
}

impl Page<'_> {
    /// Texto de la capa de texto del PDF (vacío en páginas escaneadas sin OCR previo).
    pub fn text(&self) -> String {
        unsafe {
            let tp = (self.lib.FPDFText_LoadPage)(self.page);
            if tp.is_null() {
                return String::new();
            }
            let n = (self.lib.FPDFText_CountChars)(tp);
            let mut out = String::new();
            if n > 0 {
                let mut buf = vec![0u16; n as usize + 1];
                let written = (self.lib.FPDFText_GetText)(tp, 0, n, buf.as_mut_ptr());
                let len = (written.max(1) - 1) as usize;
                out = String::from_utf16_lossy(&buf[..len.min(buf.len())]);
            }
            (self.lib.FPDFText_ClosePage)(tp);
            out
        }
    }

    /// Palabras (secuencias de letras y dígitos) de la capa de texto con su rectángulo
    /// en fracciones de la página tal como se dibuja (ya con la rotación aplicada).
    pub fn words(&self) -> Vec<crate::ocr::Word> {
        const SCALE: c_int = 100_000;
        let mut words: Vec<crate::ocr::Word> = Vec::new();
        unsafe {
            let tp = (self.lib.FPDFText_LoadPage)(self.page);
            if tp.is_null() {
                return words;
            }
            let n = (self.lib.FPDFText_CountChars)(tp);
            let mut open = false;
            for i in 0..n {
                let c = char::from_u32((self.lib.FPDFText_GetUnicode)(tp, i)).unwrap_or(' ');
                let mut rect = RectF { left: 0.0, top: 0.0, right: 0.0, bottom: 0.0 };
                if !c.is_alphanumeric() || (self.lib.FPDFText_GetLooseCharBox)(tp, i, &mut rect) == 0 {
                    open = false;
                    continue;
                }
                let corner = |x: f32, y: f32| {
                    let (mut dx, mut dy) = (0, 0);
                    (self.lib.FPDF_PageToDevice)(self.page, 0, 0, SCALE, SCALE, 0, x as c_double, y as c_double, &mut dx, &mut dy);
                    (dx as f32 / SCALE as f32, dy as f32 / SCALE as f32)
                };
                let (ax, ay) = corner(rect.left, rect.top);
                let (bx, by) = corner(rect.right, rect.bottom);
                let r = [ax.min(bx), ay.min(by), ax.max(bx), ay.max(by)];
                match words.last_mut() {
                    Some(w) if open => {
                        w.text.push(c);
                        w.rect = [w.rect[0].min(r[0]), w.rect[1].min(r[1]), w.rect[2].max(r[2]), w.rect[3].max(r[3])];
                    }
                    _ => words.push(crate::ocr::Word { text: c.to_string(), rect: r }),
                }
                open = true;
            }
            (self.lib.FPDFText_ClosePage)(tp);
        }
        words
    }

    /// Fracción del área de la página cubierta por imágenes (≈1 en un escaneo).
    pub fn image_coverage(&self) -> f32 {
        let mut covered = 0.0f32;
        let (pw, ph) = self.size();
        self.for_each_image(|_, w, h| covered += w * h);
        if pw * ph <= 0.0 {
            0.0
        } else {
            (covered / (pw * ph)).min(1.0)
        }
    }

    /// Resolución de la imagen más grande de la página (la del escaneo), si la hay.
    pub fn scan_dpi(&self) -> Option<f32> {
        let mut best: Option<(f32, f32)> = None;
        self.for_each_image(|obj, w, _| unsafe {
            let (mut px, mut py) = (0 as c_uint, 0 as c_uint);
            if w > 0.0 && (self.lib.FPDFImageObj_GetImagePixelSize)(obj, &mut px, &mut py) != 0 && best.map(|b| w > b.0).unwrap_or(true) {
                best = Some((w, px as f32 / (w / 72.0)));
            }
        });
        best.map(|b| b.1)
    }

    /// Llama a `f(objeto, ancho, alto)` por cada imagen, con su tamaño visible en puntos.
    fn for_each_image(&self, mut f: impl FnMut(Handle, f32, f32)) {
        unsafe {
            let (pw, ph) = self.size();
            let count = (self.lib.FPDFPage_CountObjects)(self.page).min(MAX_OBJECTS);
            for i in 0..count {
                let obj = (self.lib.FPDFPage_GetObject)(self.page, i);
                if obj.is_null() || (self.lib.FPDFPageObj_GetType)(obj) != FPDF_PAGEOBJ_IMAGE {
                    continue;
                }
                let (mut l, mut b, mut r, mut t) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                if (self.lib.FPDFPageObj_GetBounds)(obj, &mut l, &mut b, &mut r, &mut t) != 0 {
                    let w = (r.min(pw) - l.max(0.0)).max(0.0);
                    let h = (t.min(ph) - b.max(0.0)).max(0.0);
                    f(obj, w, h);
                }
            }
        }
    }

    pub fn has_objects(&self) -> bool {
        unsafe { (self.lib.FPDFPage_CountObjects)(self.page) > 0 }
    }

    /// Ancho y alto en puntos, ya con la rotación de la página aplicada.
    pub fn size(&self) -> (f32, f32) {
        unsafe { ((self.lib.FPDF_GetPageWidthF)(self.page), (self.lib.FPDF_GetPageHeightF)(self.page)) }
    }

    /// Dibuja la página en escala de grises a `dpi`, sin exceder `max_side` píxeles por lado.
    pub fn render_gray(&self, dpi: f32, max_side: f32) -> Result<Gray, String> {
        let (width, height, pixels) = self.render(dpi, max_side, FPDF_BITMAP_GRAY, 1)?;
        Ok(Gray { width, height, pixels })
    }

    /// Dibuja la página a color (BGR) a `dpi`, sin exceder `max_side` píxeles por lado.
    pub fn render_color(&self, dpi: f32, max_side: f32) -> Result<Color, String> {
        let (width, height, pixels) = self.render(dpi, max_side, FPDF_BITMAP_BGR, 3)?;
        Ok(Color { width, height, pixels })
    }

    fn render(&self, dpi: f32, max_side: f32, format: c_int, bpp: i32) -> Result<(i32, i32, Vec<u8>), String> {
        let (pw, ph) = self.size();
        if pw <= 0.0 || ph <= 0.0 {
            return Err("Página sin dimensiones".into());
        }
        let mut scale = dpi / 72.0;
        let longest = pw.max(ph) * scale;
        if longest > max_side {
            scale *= max_side / longest;
        }
        let width = ((pw * scale).round() as i32).max(1);
        let height = ((ph * scale).round() as i32).max(1);
        // PDFium exige filas alineadas a 4 bytes
        let row = width * bpp;
        let stride = (row + 3) & !3;
        let mut buf = vec![0xFFu8; stride as usize * height as usize];
        unsafe {
            let bmp = (self.lib.FPDFBitmap_CreateEx)(width, height, format, buf.as_mut_ptr() as *mut c_void, stride);
            if bmp.is_null() {
                return Err("No se pudo crear el mapa de bits".into());
            }
            (self.lib.FPDFBitmap_FillRect)(bmp, 0, 0, width, height, 0xFFFF_FFFF);
            (self.lib.FPDF_RenderPageBitmap)(bmp, self.page, 0, 0, width, height, 0, FPDF_ANNOT);
            (self.lib.FPDFBitmap_Destroy)(bmp);
        }
        if stride != row {
            let mut tight = Vec::with_capacity(row as usize * height as usize);
            for r in buf.chunks_exact(stride as usize) {
                tight.extend_from_slice(&r[..row as usize]);
            }
            buf = tight;
        }
        Ok((width, height, buf))
    }
}

impl Drop for Page<'_> {
    fn drop(&mut self) {
        unsafe { (self.lib.FPDF_ClosePage)(self.page) }
    }
}
