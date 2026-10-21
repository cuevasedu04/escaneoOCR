//! Detección de tablas en una página escaneada a partir de sus líneas horizontales.
//!
//! Se buscan las líneas de la cuadrícula (renglones oscuros que cruzan buena parte de la página),
//! corrigiendo la ligera inclinación típica de un escaneo. Las franjas entre líneas consecutivas
//! son las filas; no depende de que el OCR lea bien el texto de cada fila.

/// Píxel que se considera tinta.
const DARK: u8 = 140;
/// Inclinaciones que se prueban: ±2° en pasos de 0.25°.
const SKEW_STEPS: i32 = 8;
const SKEW_STEP_DEG: f32 = 0.25;
/// Una línea de tabla cubre al menos esta fracción del ancho de la página.
const LINE_MIN_WIDTH: f32 = 0.25;
/// Separación máxima entre líneas de un mismo grupo (fracción del alto).
const GROUP_GAP: f32 = 0.15;
/// Altura mínima de una fila (fracción del alto); menos es una línea doble.
const MIN_ROW_HEIGHT: f32 = 0.008;
/// Cobertura del ancho de la tabla de una línea completa, y mínima de una línea cortada.
const FULL_COVERAGE: f32 = 0.75;
const PARTIAL_COVERAGE: f32 = 0.3;
/// Mínimo de líneas para considerar que hay una tabla (tres filas).
const MIN_LINES: usize = 4;

/// Franja de una fila en coordenadas «enderezadas»: y' = y − pendiente·x, en píxeles.
#[derive(Clone, Copy)]
pub struct Band {
    pub top: f32,
    pub bottom: f32,
    pub left: f32,
    pub right: f32,
}

pub struct Table {
    /// La primera es el encabezado de la tabla.
    pub bands: Vec<Band>,
}

pub struct Layout {
    pub slope: f32,
    pub tables: Vec<Table>,
    width: f32,
    height: f32,
}

struct Line {
    start: f32,
    end: f32,
    left: f32,
    right: f32,
}

pub fn detect(gray: &[u8], width: usize, height: usize) -> Layout {
    let (w, h) = (width as f32, height as f32);
    // Tinta muestreada en columnas alternas: suficiente para líneas y la mitad del trabajo
    let mut points: Vec<(u32, u32)> = Vec::new();
    for y in 0..height {
        let row = &gray[y * width..(y + 1) * width];
        for x in (0..width).step_by(2) {
            if row[x] < DARK {
                points.push((x as u32, y as u32));
            }
        }
    }
    let max_tan = ((SKEW_STEPS as f32) * SKEW_STEP_DEG).to_radians().tan();
    let offset = (max_tan * w).ceil() as i64 + 2;
    let bins = height + 2 * offset as usize + 2;

    // La inclinación correcta concentra la tinta de las líneas en pocos renglones
    let histogram = |slope: f32| {
        let mut hist = vec![0u32; bins];
        for &(x, y) in &points {
            let idx = (y as f32 - slope * x as f32).round() as i64 + offset;
            if idx >= 0 && (idx as usize) < bins {
                hist[idx as usize] += 1;
            }
        }
        hist
    };
    let mut best = (0.0f32, 0u64);
    for step in -SKEW_STEPS..=SKEW_STEPS {
        let slope = (step as f32 * SKEW_STEP_DEG).to_radians().tan();
        let score: u64 = histogram(slope).iter().map(|&c| (c as u64) * (c as u64)).sum();
        if score > best.1 {
            best = (slope, score);
        }
    }
    let slope = best.0;
    let hist = histogram(slope);

    // Renglones (enderezados) con suficiente tinta seguida = líneas de tabla
    let threshold = (LINE_MIN_WIDTH * w / 2.0) as u32;
    let mut segments: Vec<(usize, usize)> = Vec::new();
    for (i, &c) in hist.iter().enumerate() {
        if c < threshold {
            continue;
        }
        match segments.last_mut() {
            Some(last) if i <= last.1 + 3 => last.1 = i,
            _ => segments.push((i, i)),
        }
    }
    // Extremos horizontales de cada línea
    let mut owner = vec![usize::MAX; bins];
    for (n, &(s, e)) in segments.iter().enumerate() {
        for slot in owner.iter_mut().take((e + 2).min(bins)).skip(s.saturating_sub(1)) {
            *slot = n;
        }
    }
    let mut xs: Vec<Vec<u32>> = vec![Vec::new(); segments.len()];
    for &(x, y) in &points {
        let idx = (y as f32 - slope * x as f32).round() as i64 + offset;
        if idx >= 0 && (idx as usize) < bins && owner[idx as usize] != usize::MAX {
            xs[owner[idx as usize]].push(x);
        }
    }
    // Extremos de cada línea: el tramo continuo más largo, para no contar marcas sueltas en el margen
    const CELL: usize = 8;
    const MAX_HOLE: usize = 6;
    let cells = width / CELL + 1;
    let mut lines: Vec<Line> = Vec::new();
    for (n, &(s, e)) in segments.iter().enumerate() {
        let mut filled = vec![false; cells];
        for &x in &xs[n] {
            filled[x as usize / CELL] = true;
        }
        let (mut best, mut run_start, mut last_filled) = ((0usize, 0usize), None::<usize>, 0usize);
        for (c, &on) in filled.iter().enumerate() {
            if !on {
                continue;
            }
            match run_start {
                Some(_) if c - last_filled <= MAX_HOLE + 1 => {}
                _ => run_start = Some(c),
            }
            last_filled = c;
            let start = run_start.unwrap();
            if c - start > best.1 - best.0 {
                best = (start, c);
            }
        }
        let (left, right) = ((best.0 * CELL) as f32, ((best.1 + 1) * CELL) as f32);
        if right - left >= LINE_MIN_WIDTH * w {
            lines.push(Line { start: (s as i64 - offset) as f32, end: (e as i64 - offset) as f32, left, right });
        }
    }

    // Líneas poco separadas forman un grupo. Dentro de él, las líneas que cruzan casi todo el ancho
    // definen la tabla (así se descartan recuadros vecinos, como el de la unidad administrativa);
    // las líneas cortadas por el escaneo o por las firmas también separan filas si caen en su lugar.
    let mut tables = Vec::new();
    let mut groups: Vec<Vec<&Line>> = Vec::new();
    for line in &lines {
        match groups.last_mut() {
            Some(g) if line.start - g.last().unwrap().end <= GROUP_GAP * h => g.push(line),
            _ => groups.push(vec![line]),
        }
    }
    for group in groups {
        if group.len() < MIN_LINES {
            continue;
        }
        let median = |mut v: Vec<f32>| {
            v.sort_by(f32::total_cmp);
            v[v.len() / 2]
        };
        // Bordes de la tabla: los de sus líneas más largas (el marco suele salir completo, mientras
        // que las líneas interiores a veces se pierden en un extremo del escaneo)
        let widest = group.iter().map(|l| l.right - l.left).fold(0.0f32, f32::max);
        let long = || group.iter().filter(|l| l.right - l.left >= FULL_COVERAGE * widest);
        let left = long().map(|l| l.left).fold(f32::MAX, f32::min);
        let right = long().map(|l| l.right).fold(0.0f32, f32::max);
        let coverage = |l: &Line| (l.right.min(right) - l.left.max(left)) / (right - left);
        let full: Vec<&Line> = group.iter().copied().filter(|l| coverage(l) >= FULL_COVERAGE).collect();
        if full.len() < 3 {
            continue;
        }
        let row = median(full.windows(2).map(|p| p[1].start - p[0].end).collect());
        let (top, bottom) = (full[0].start, full[full.len() - 1].end);
        let mut seps: Vec<&Line> = group
            .iter()
            .copied()
            .filter(|l| {
                let c = coverage(l);
                c >= FULL_COVERAGE
                    || (c >= PARTIAL_COVERAGE
                        && l.start > top
                        && l.start < bottom
                        // Una «línea» a media fila es escritura, no un borde
                        && full.iter().all(|f| (l.start - f.start).abs() >= 0.6 * row))
            })
            .collect();
        seps.sort_by(|a, b| a.start.total_cmp(&b.start));
        // La línea inferior a veces sale incompleta: se acepta si está a una fila de distancia
        for &l in &group {
            let last = seps[seps.len() - 1];
            if l.start > last.end && coverage(l) >= PARTIAL_COVERAGE && l.start - last.end <= 1.6 * row {
                seps.push(l);
            }
        }

        let mut bands = Vec::new();
        for (n, pair) in seps.windows(2).enumerate() {
            let (top, bottom) = (pair[0].end + 1.0, pair[1].start - 1.0);
            let height = bottom - top;
            if height < MIN_ROW_HEIGHT * h {
                continue;
            }
            // Si falta una línea, la franja abarca varias filas: se reparte por la altura típica
            let parts = if n > 0 && height > 1.6 * row { (height / row).round().max(1.0) as usize } else { 1 };
            let step = height / parts as f32;
            for k in 0..parts {
                bands.push(Band { top: top + step * k as f32, bottom: top + step * (k + 1) as f32, left, right });
            }
        }
        if bands.len() >= 2 {
            tables.push(Table { bands });
        }
    }

    Layout { slope, tables, width: w, height: h }
}

impl Layout {
    /// Tabla y franja que contienen el punto (en fracciones de la página). Con `any_x`, basta con
    /// que el punto esté a la altura de la franja, aunque quede fuera de los bordes de la tabla.
    pub fn band_at(&self, u: f32, v: f32, any_x: bool) -> Option<(usize, usize)> {
        let (x, y) = (u * self.width, v * self.height);
        let yd = y - self.slope * x;
        for (t, table) in self.tables.iter().enumerate() {
            for (b, band) in table.bands.iter().enumerate() {
                if yd >= band.top && yd <= band.bottom && (any_x || (x >= band.left && x <= band.right)) {
                    return Some((t, b));
                }
            }
        }
        None
    }

    /// Rectángulos (en fracciones de la página) que cubren la franja, en tiras verticales
    /// si la página está inclinada para seguir la fila sin invadir las vecinas.
    pub fn band_rects(&self, band: &Band) -> Vec<[f32; 4]> {
        let drift = (self.slope * (band.right - band.left)).abs();
        let strips = if drift <= 2.0 { 1 } else { ((band.right - band.left) / (0.02 * self.width)).ceil().max(1.0) as usize };
        let step = (band.right - band.left) / strips as f32;
        (0..strips)
            .map(|i| {
                let (x0, x1) = (band.left + step * i as f32, band.left + step * (i + 1) as f32);
                let (d0, d1) = (self.slope * x0, self.slope * x1);
                [
                    x0 / self.width,
                    (band.top + d0.min(d1)) / self.height,
                    x1 / self.width,
                    (band.bottom + d0.max(d1)) / self.height,
                ]
            })
            .collect()
    }
}
