//! SVG path parsing, curve flattening, transforms, stroke expansion, and built-in vector icons.

use sse_core::{Error, Result};
use std::f32::consts::PI;

const MAX_PATH_VERBS: usize = 1_000_000;
const MAX_FLATTEN_DEPTH: u8 = 18;
const ROUND_STEPS: u16 = 16;

/// A two-dimensional point.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    /// X coordinate.
    pub x: f32,
    /// Y coordinate.
    pub y: f32,
}

impl Point {
    /// Creates a point.
    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self { Self { x, y } }
}

/// A straight segment consumed by the coverage rasteriser.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    /// Segment start.
    pub from: Point,
    /// Segment end.
    pub to: Point,
}

/// Fill rule for path coverage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FillRule {
    /// Count all signed edge crossings.
    NonZero,
    /// Toggle coverage at every crossing.
    EvenOdd,
}

/// An affine SVG-style transform matrix.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transform {
    /// Matrix xx.
    pub a: f32,
    /// Matrix yx.
    pub b: f32,
    /// Matrix xy.
    pub c: f32,
    /// Matrix yy.
    pub d: f32,
    /// Translation x.
    pub e: f32,
    /// Translation y.
    pub f: f32,
}

impl Transform {
    /// Identity transform.
    #[must_use]
    pub const fn identity() -> Self { Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 } }
    /// Translation transform.
    #[must_use]
    pub const fn translate(x: f32, y: f32) -> Self { Self { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: x, f: y } }
    /// Scale transform.
    #[must_use]
    pub const fn scale(x: f32, y: f32) -> Self { Self { a: x, b: 0.0, c: 0.0, d: y, e: 0.0, f: 0.0 } }
    /// Applies the transform to a point.
    #[must_use]
    pub fn apply(self, point: Point) -> Point {
        Point::new(self.a.mul_add(point.x, self.c.mul_add(point.y, self.e)), self.b.mul_add(point.x, self.d.mul_add(point.y, self.f)))
    }
    /// Concatenates `other` after this transform.
    #[must_use]
    pub fn then(self, other: Self) -> Self {
        Self {
            a: other.a.mul_add(self.a, other.c * self.b),
            b: other.b.mul_add(self.a, other.d * self.b),
            c: other.a.mul_add(self.c, other.c * self.d),
            d: other.b.mul_add(self.c, other.d * self.d),
            e: other.a.mul_add(self.e, other.c.mul_add(self.f, other.e)),
            f: other.b.mul_add(self.e, other.d.mul_add(self.f, other.f)),
        }
    }
}

/// One retained path verb.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verb {
    /// Starts a subpath.
    Move(Point),
    /// Straight line.
    Line(Point),
    /// Quadratic Bézier.
    Quad { ctrl: Point, to: Point },
    /// Cubic Bézier.
    Cubic { ctrl1: Point, ctrl2: Point, to: Point },
    /// Closes the current subpath.
    Close,
}

/// Parsed vector path.
#[derive(Debug, Clone, PartialEq)]
pub struct Path {
    verbs: Vec<Verb>,
    rule: FillRule,
}

impl Path {
    /// Creates an empty path.
    #[must_use]
    pub fn new(rule: FillRule) -> Self { Self { verbs: Vec::new(), rule } }
    /// Fill rule.
    #[must_use]
    pub const fn fill_rule(&self) -> FillRule { self.rule }
    /// Retained verbs.
    #[must_use]
    pub fn verbs(&self) -> &[Verb] { &self.verbs }
    /// Changes the fill rule.
    pub fn set_fill_rule(&mut self, rule: FillRule) { self.rule = rule; }
    /// Applies an affine transform in place.
    pub fn transform(&mut self, transform: Transform) {
        for verb in &mut self.verbs {
            *verb = match *verb {
                Verb::Move(point) => Verb::Move(transform.apply(point)),
                Verb::Line(point) => Verb::Line(transform.apply(point)),
                Verb::Quad { ctrl, to } => Verb::Quad { ctrl: transform.apply(ctrl), to: transform.apply(to) },
                Verb::Cubic { ctrl1, ctrl2, to } => Verb::Cubic { ctrl1: transform.apply(ctrl1), ctrl2: transform.apply(ctrl2), to: transform.apply(to) },
                Verb::Close => Verb::Close,
            };
        }
    }
    /// Flattens all curves into straight coverage segments.
    pub fn flatten(&self, tolerance: f32) -> Result<Vec<Segment>> {
        if !tolerance.is_finite() || tolerance <= 0.0 { return Err(Error::damaged("flatten tolerance must be finite and positive")); }
        let subpaths = flatten_subpaths(self, tolerance)?;
        let mut output = Vec::new();
        for subpath in subpaths {
            for pair in subpath.points.windows(2) {
                let Some(from) = pair.first().copied() else { continue; };
                let Some(to) = pair.get(1).copied() else { continue; };
                if from != to { output.push(Segment { from, to }); }
            }
            if subpath.closed {
                let Some(from) = subpath.points.last().copied() else { continue; };
                let Some(to) = subpath.points.first().copied() else { continue; };
                if from != to { output.push(Segment { from, to }); }
            }
        }
        Ok(output)
    }
}

/// Line-cap style for stroke expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineCap { /// Stops at the endpoint. 
    Butt, /// Semicircular endpoint.
    Round, /// Extends by half the line width.
    Square }

/// Line-join style for stroke expansion.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineJoin { /// Sharp intersection limited by `miter_limit`.
    Miter, /// Circular join.
    Round, /// Cut-off corner.
    Bevel }

/// Stroke parameters.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StrokeStyle {
    /// Stroke width.
    pub width: f32,
    /// Endpoint cap.
    pub cap: LineCap,
    /// Segment join.
    pub join: LineJoin,
    /// Maximum miter length divided by half-width.
    pub miter_limit: f32,
}

impl Default for StrokeStyle {
    fn default() -> Self { Self { width: 2.0, cap: LineCap::Butt, join: LineJoin::Miter, miter_limit: 4.0 } }
}

/// Filled line work produced by stroke expansion.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeFill {
    /// Boundary segments. Overlapping polygons intentionally remain separate; non-zero filling unions them naturally.
    pub segments: Vec<Segment>,
    /// Stroke expansion always uses non-zero winding.
    pub rule: FillRule,
}

/// Parses SVG path data including implicit command repetition and elliptical arcs.
pub fn parse_svg_path(data: &str) -> Result<Path> {
    let mut scanner = Scanner::new(data);
    let mut path = Path::new(FillRule::NonZero);
    let mut current = Point::new(0.0, 0.0);
    let mut start = current;
    let mut previous: Option<u8> = None;
    let mut last_cubic: Option<Point> = None;
    let mut last_quad: Option<Point> = None;
    while scanner.has_more() {
        let command = if scanner.peek_command().is_some() { scanner.command()? } else { previous.ok_or_else(|| Error::damaged("SVG path starts with numbers instead of a command"))? };
        previous = Some(command);
        let relative = command.is_ascii_lowercase();
        let upper = command.to_ascii_uppercase();
        match upper {
            b'M' => {
                let mut first = true;
                while scanner.has_number() {
                    let point = scanner.point()?;
                    let target = resolve(point, current, relative);
                    if first { push_verb(&mut path, Verb::Move(target))?; start = target; first = false; } else { push_verb(&mut path, Verb::Line(target))?; }
                    current = target; last_cubic = None; last_quad = None;
                }
                if first { return Err(Error::damaged("SVG M command lacks coordinates")); }
                previous = Some(if relative { b'l' } else { b'L' });
            }
            b'L' => { require_number(&scanner, "L")?; while scanner.has_number() { let target=resolve(scanner.point()?,current,relative);push_verb(&mut path,Verb::Line(target))?;current=target;last_cubic=None;last_quad=None; } }
            b'H' => { require_number(&scanner, "H")?; while scanner.has_number() { let value=scanner.number()?;let x=if relative{current.x+value}else{value};let target=Point::new(x,current.y);push_verb(&mut path,Verb::Line(target))?;current=target;last_cubic=None;last_quad=None; } }
            b'V' => { require_number(&scanner, "V")?; while scanner.has_number() { let value=scanner.number()?;let y=if relative{current.y+value}else{value};let target=Point::new(current.x,y);push_verb(&mut path,Verb::Line(target))?;current=target;last_cubic=None;last_quad=None; } }
            b'C' => { require_number(&scanner, "C")?; while scanner.has_number() { let c1=resolve(scanner.point()?,current,relative);let c2=resolve(scanner.point()?,current,relative);let to=resolve(scanner.point()?,current,relative);push_verb(&mut path,Verb::Cubic{ctrl1:c1,ctrl2:c2,to})?;current=to;last_cubic=Some(c2);last_quad=None; } }
            b'S' => { require_number(&scanner, "S")?; while scanner.has_number() { let c1=last_cubic.map_or(current,|point|reflect(point,current));let c2=resolve(scanner.point()?,current,relative);let to=resolve(scanner.point()?,current,relative);push_verb(&mut path,Verb::Cubic{ctrl1:c1,ctrl2:c2,to})?;current=to;last_cubic=Some(c2);last_quad=None; } }
            b'Q' => { require_number(&scanner, "Q")?; while scanner.has_number() { let ctrl=resolve(scanner.point()?,current,relative);let to=resolve(scanner.point()?,current,relative);push_verb(&mut path,Verb::Quad{ctrl,to})?;current=to;last_quad=Some(ctrl);last_cubic=None; } }
            b'T' => { require_number(&scanner, "T")?; while scanner.has_number() { let ctrl=last_quad.map_or(current,|point|reflect(point,current));let to=resolve(scanner.point()?,current,relative);push_verb(&mut path,Verb::Quad{ctrl,to})?;current=to;last_quad=Some(ctrl);last_cubic=None; } }
            b'A' => { require_number(&scanner, "A")?; while scanner.has_number() { let rx=scanner.number()?.abs();let ry=scanner.number()?.abs();let rotation=scanner.number()?;let large=scanner.flag()?;let sweep=scanner.flag()?;let to=resolve(scanner.point()?,current,relative);let cubics=arc_to_cubics(current,rx,ry,rotation,large,sweep,to)?;if cubics.is_empty(){push_verb(&mut path,Verb::Line(to))?;}else{for cubic in cubics{push_verb(&mut path,Verb::Cubic{ctrl1:cubic.ctrl1,ctrl2:cubic.ctrl2,to:cubic.to})?;}}current=to;last_cubic=None;last_quad=None; } }
            b'Z' => { push_verb(&mut path,Verb::Close)?;current=start;last_cubic=None;last_quad=None;previous=None; }
            _ => return Err(Error::damaged(format!("unsupported SVG path command {}", char::from(command)))),
        }
    }
    Ok(path)
}

/// Expands a path stroke into fillable boundary segments.
pub fn stroke_to_fill(path: &Path, style: StrokeStyle, tolerance: f32) -> Result<StrokeFill> {
    if !style.width.is_finite() || style.width <= 0.0 || !style.miter_limit.is_finite() || style.miter_limit <= 0.0 { return Err(Error::damaged("stroke width and miter limit must be positive finite values")); }
    let subpaths = flatten_subpaths(path, tolerance)?;
    let half = style.width * 0.5;
    let mut segments = Vec::new();
    for subpath in subpaths {
        if subpath.points.len() < 2 { continue; }
        for pair in subpath.points.windows(2) {
            let Some(a)=pair.first().copied() else{continue}; let Some(b)=pair.get(1).copied() else{continue}; add_stroke_quad(&mut segments,a,b,half)?;
        }
        if subpath.closed {
            let Some(a)=subpath.points.last().copied() else{continue};let Some(b)=subpath.points.first().copied() else{continue};add_stroke_quad(&mut segments,a,b,half)?;
        }
        add_joins(&mut segments,&subpath,style,half)?;
        if !subpath.closed { add_caps(&mut segments,&subpath,style,half)?; }
    }
    Ok(StrokeFill { segments, rule: FillRule::NonZero })
}

#[derive(Debug,Clone,Copy)]
struct CubicArc { ctrl1:Point, ctrl2:Point, to:Point }

fn arc_to_cubics(from:Point,mut rx:f32,mut ry:f32,rotation_degrees:f32,large:bool,sweep:bool,to:Point)->Result<Vec<CubicArc>>{
    if !rx.is_finite()||!ry.is_finite()||!rotation_degrees.is_finite()||!finite_point(from)||!finite_point(to){return Err(Error::damaged("non-finite SVG arc parameter"));}
    if from==to{return Ok(Vec::new())} if rx==0.0||ry==0.0{return Ok(Vec::new())}
    let phi=rotation_degrees*PI/180.0;let (sin_phi,cos_phi)=phi.sin_cos();let dx=(from.x-to.x)*0.5;let dy=(from.y-to.y)*0.5;let x1p=cos_phi.mul_add(dx,sin_phi*dy);let y1p=(-sin_phi).mul_add(dx,cos_phi*dy);rx=rx.abs();ry=ry.abs();
    let mut rx2=rx*rx;let mut ry2=ry*ry;let x2=x1p*x1p;let y2=y1p*y1p;let lambda=x2/rx2+y2/ry2;if lambda>1.0{let scale=lambda.sqrt();rx*=scale;ry*=scale;rx2=rx*rx;ry2=ry*ry;}
    let numerator=(rx2*ry2-rx2*y2-ry2*x2).max(0.0);let denominator=(rx2*y2+ry2*x2).max(f32::MIN_POSITIVE);let sign=if large==sweep{-1.0}else{1.0};let coefficient=sign*(numerator/denominator).sqrt();let cxp=coefficient*(rx*y1p/ry);let cyp=coefficient*(-ry*x1p/rx);let midpoint=Point::new((from.x+to.x)*0.5,(from.y+to.y)*0.5);let cx=cos_phi.mul_add(cxp,-sin_phi*cyp)+midpoint.x;let cy=sin_phi.mul_add(cxp,cos_phi*cyp)+midpoint.y;
    let ux=(x1p-cxp)/rx;let uy=(y1p-cyp)/ry;let vx=(-x1p-cxp)/rx;let vy=(-y1p-cyp)/ry;let theta1=uy.atan2(ux);let mut delta=(ux*vy-uy*vx).atan2(ux*vx+uy*vy);if !sweep&&delta>0.0{delta-=2.0*PI;}else if sweep&&delta<0.0{delta+=2.0*PI;}
    let mut count=1_u16;let mut remaining=delta.abs();while remaining>PI*0.5&&count<4{count=count.saturating_add(1);remaining-=PI*0.5;}let step=delta/f32::from(count);let alpha=(4.0/3.0)*(step*0.25).tan();let mut cubics=Vec::with_capacity(usize::from(count));
    for index in 0..count { let t1=theta1+step*f32::from(index);let t2=t1+step;let (s1,c1)=t1.sin_cos();let (s2,c2)=t2.sin_cos();let p1=ellipse_point(cx,cy,rx,ry,cos_phi,sin_phi,c1,s1);let p2=ellipse_point(cx,cy,rx,ry,cos_phi,sin_phi,c2,s2);let d1=ellipse_tangent(rx,ry,cos_phi,sin_phi,c1,s1);let d2=ellipse_tangent(rx,ry,cos_phi,sin_phi,c2,s2);let ctrl1=Point::new(p1.x+alpha*d1.x,p1.y+alpha*d1.y);let ctrl2=Point::new(p2.x-alpha*d2.x,p2.y-alpha*d2.y);cubics.push(CubicArc{ctrl1,ctrl2,to:p2}); }
    if let Some(last)=cubics.last_mut(){last.to=to;} Ok(cubics)
}
fn ellipse_point(cx:f32,cy:f32,rx:f32,ry:f32,cos:f32,sin:f32,ct:f32,st:f32)->Point{Point::new(cx+cos*rx*ct-sin*ry*st,cy+sin*rx*ct+cos*ry*st)}
fn ellipse_tangent(rx:f32,ry:f32,cos:f32,sin:f32,ct:f32,st:f32)->Point{Point::new(-cos*rx*st-sin*ry*ct,-sin*rx*st+cos*ry*ct)}

#[derive(Debug)] struct FlatSubpath{points:Vec<Point>,closed:bool}
fn flatten_subpaths(path:&Path,tolerance:f32)->Result<Vec<FlatSubpath>>{if !tolerance.is_finite()||tolerance<=0.0{return Err(Error::damaged("flatten tolerance must be positive"));}let mut result=Vec::new();let mut current=Point::new(0.0,0.0);let mut active:Option<FlatSubpath>=None;for verb in &path.verbs{match *verb{Verb::Move(p)=>{flush_subpath(&mut result,&mut active);active=Some(FlatSubpath{points:vec![p],closed:false});current=p;}Verb::Line(p)=>{ensure_active(&mut active,current);if let Some(s)=active.as_mut(){s.points.push(p);}current=p;}Verb::Quad{ctrl,to}=>{ensure_active(&mut active,current);if let Some(s)=active.as_mut(){flatten_quad_points(current,ctrl,to,tolerance,0,&mut s.points)?;}current=to;}Verb::Cubic{ctrl1,ctrl2,to}=>{ensure_active(&mut active,current);if let Some(s)=active.as_mut(){flatten_cubic_points(current,ctrl1,ctrl2,to,tolerance,0,&mut s.points)?;}current=to;}Verb::Close=>{if let Some(s)=active.as_mut(){s.closed=true;if let Some(first)=s.points.first().copied(){current=first;}}flush_subpath(&mut result,&mut active);}}}flush_subpath(&mut result,&mut active);Ok(result)}
fn ensure_active(active:&mut Option<FlatSubpath>,current:Point){if active.is_none(){*active=Some(FlatSubpath{points:vec![current],closed:false});}}
fn flush_subpath(result:&mut Vec<FlatSubpath>,active:&mut Option<FlatSubpath>){if let Some(path)=active.take(){if !path.points.is_empty(){result.push(path);}}}
fn flatten_quad_points(p0:Point,p1:Point,p2:Point,tol:f32,depth:u8,out:&mut Vec<Point>)->Result<()>{if depth>=MAX_FLATTEN_DEPTH||quad_flat_enough(p0,p1,p2,tol){out.push(p2);return Ok(())}let a=mid(p0,p1);let b=mid(p1,p2);let m=mid(a,b);let next=depth.checked_add(1).ok_or_else(||Error::damaged("quadratic flatten depth overflow"))?;flatten_quad_points(p0,a,m,tol,next,out)?;flatten_quad_points(m,b,p2,tol,next,out)}
fn flatten_cubic_points(p0:Point,p1:Point,p2:Point,p3:Point,tol:f32,depth:u8,out:&mut Vec<Point>)->Result<()>{if depth>=MAX_FLATTEN_DEPTH||cubic_flat_enough(p0,p1,p2,p3,tol){out.push(p3);return Ok(())}let a=mid(p0,p1);let b=mid(p1,p2);let c=mid(p2,p3);let d=mid(a,b);let e=mid(b,c);let m=mid(d,e);let next=depth.checked_add(1).ok_or_else(||Error::damaged("cubic flatten depth overflow"))?;flatten_cubic_points(p0,a,d,m,tol,next,out)?;flatten_cubic_points(m,e,c,p3,tol,next,out)}
fn quad_flat_enough(p0:Point,p1:Point,p2:Point,tol:f32)->bool{distance_to_line(p1,p0,p2)<=tol}
fn cubic_flat_enough(p0:Point,p1:Point,p2:Point,p3:Point,tol:f32)->bool{distance_to_line(p1,p0,p3).max(distance_to_line(p2,p0,p3))<=tol}
fn distance_to_line(p:Point,a:Point,b:Point)->f32{let dx=b.x-a.x;let dy=b.y-a.y;let length=(dx*dx+dy*dy).sqrt();if length<=f32::EPSILON{return ((p.x-a.x)*(p.x-a.x)+(p.y-a.y)*(p.y-a.y)).sqrt()}((dy*(p.x-a.x)-dx*(p.y-a.y)).abs())/length}
fn mid(a:Point,b:Point)->Point{Point::new((a.x+b.x)*0.5,(a.y+b.y)*0.5)}
fn reflect(point:Point,around:Point)->Point{Point::new(around.x*2.0-point.x,around.y*2.0-point.y)}
fn resolve(point:Point,current:Point,relative:bool)->Point{if relative{Point::new(current.x+point.x,current.y+point.y)}else{point}}
fn finite_point(point:Point)->bool{point.x.is_finite()&&point.y.is_finite()}
fn push_verb(path:&mut Path,verb:Verb)->Result<()> {if path.verbs.len()>=MAX_PATH_VERBS{return Err(Error::Refused("SVG path exceeds verb limit".to_owned()));}path.verbs.push(verb);Ok(())}
fn require_number(scanner:&Scanner<'_>,name:&str)->Result<()> {if scanner.has_number(){Ok(())}else{Err(Error::damaged(format!("SVG {name} command lacks parameters")))}}

fn add_stroke_quad(out:&mut Vec<Segment>,a:Point,b:Point,half:f32)->Result<()> {let Some((nx,ny))=normal(a,b,half)else{return Ok(())};add_polygon(out,&[Point::new(a.x+nx,a.y+ny),Point::new(b.x+nx,b.y+ny),Point::new(b.x-nx,b.y-ny),Point::new(a.x-nx,a.y-ny)])}
fn normal(a:Point,b:Point,scale:f32)->Option<(f32,f32)>{let dx=b.x-a.x;let dy=b.y-a.y;let length=(dx*dx+dy*dy).sqrt();if length<=f32::EPSILON{return None}Some((-dy/length*scale,dx/length*scale))}
fn add_polygon(out:&mut Vec<Segment>,points:&[Point])->Result<()> {if points.len()<3{return Ok(())}for pair in points.windows(2){let Some(a)=pair.first().copied()else{continue};let Some(b)=pair.get(1).copied()else{continue};if a!=b{out.push(Segment{from:a,to:b});}}let Some(last)=points.last().copied()else{return Ok(())};let Some(first)=points.first().copied()else{return Ok(())};if last!=first{out.push(Segment{from:last,to:first});}Ok(())}
fn add_circle(out:&mut Vec<Segment>,center:Point,radius:f32)->Result<()> {let mut points=Vec::with_capacity(usize::from(ROUND_STEPS));for step in 0..ROUND_STEPS{let angle=2.0*PI*f32::from(step)/f32::from(ROUND_STEPS);let (sin,cos)=angle.sin_cos();points.push(Point::new(center.x+cos*radius,center.y+sin*radius));}add_polygon(out,&points)}
fn add_joins(out:&mut Vec<Segment>,subpath:&FlatSubpath,style:StrokeStyle,half:f32)->Result<()> {let len=subpath.points.len();if len<3{return Ok(())}let end=if subpath.closed{len}else{len.saturating_sub(1)};let mut index=if subpath.closed{0}else{1};while index<end{let prev_index=if index==0{len.saturating_sub(1)}else{index.saturating_sub(1)};let next_index=if index.saturating_add(1)>=len{0}else{index.saturating_add(1)};let Some(prev)=subpath.points.get(prev_index).copied()else{break};let Some(center)=subpath.points.get(index).copied()else{break};let Some(next)=subpath.points.get(next_index).copied()else{break};add_join(out,prev,center,next,style,half)?;index=index.saturating_add(1);}Ok(())}
fn add_join(out:&mut Vec<Segment>,prev:Point,center:Point,next:Point,style:StrokeStyle,half:f32)->Result<()> {if style.join==LineJoin::Round{return add_circle(out,center,half)}let Some((n1x,n1y))=normal(prev,center,half)else{return Ok(())};let Some((n2x,n2y))=normal(center,next,half)else{return Ok(())};let dx1=center.x-prev.x;let dy1=center.y-prev.y;let dx2=next.x-center.x;let dy2=next.y-center.y;let cross=dx1*dy2-dy1*dx2;if cross.abs()<=f32::EPSILON{return Ok(())}let side=if cross>0.0{1.0}else{-1.0};let a=Point::new(center.x+n1x*side,center.y+n1y*side);let b=Point::new(center.x+n2x*side,center.y+n2y*side);if style.join==LineJoin::Bevel{return add_polygon(out,&[a,b,center])}let d1=Point::new(dx1,dy1);let d2=Point::new(dx2,dy2);if let Some(miter)=line_intersection(a,d1,b,d2){let dist=((miter.x-center.x)*(miter.x-center.x)+(miter.y-center.y)*(miter.y-center.y)).sqrt();if dist<=half*style.miter_limit{return add_polygon(out,&[a,miter,b,center])}}add_polygon(out,&[a,b,center])}
fn line_intersection(a:Point,da:Point,b:Point,db:Point)->Option<Point>{let denom=da.x*db.y-da.y*db.x;if denom.abs()<=f32::EPSILON{return None}let bx=b.x-a.x;let by=b.y-a.y;let t=(bx*db.y-by*db.x)/denom;Some(Point::new(a.x+da.x*t,a.y+da.y*t))}
fn add_caps(out:&mut Vec<Segment>,subpath:&FlatSubpath,style:StrokeStyle,half:f32)->Result<()> {let Some(start)=subpath.points.first().copied()else{return Ok(())};let Some(start_next)=subpath.points.get(1).copied()else{return Ok(())};let Some(end)=subpath.points.last().copied()else{return Ok(())};let Some(end_prev)=subpath.points.get(subpath.points.len().saturating_sub(2)).copied()else{return Ok(())};match style.cap{LineCap::Butt=>Ok(()),LineCap::Round=>{add_circle(out,start,half)?;add_circle(out,end,half)},LineCap::Square=>{add_square_cap(out,start,start_next,half,true)?;add_square_cap(out,end_prev,end,half,false)}}}
fn add_square_cap(out:&mut Vec<Segment>,a:Point,b:Point,half:f32,start:bool)->Result<()> {let dx=b.x-a.x;let dy=b.y-a.y;let length=(dx*dx+dy*dy).sqrt();if length<=f32::EPSILON{return Ok(())}let ux=dx/length;let uy=dy/length;let Some((nx,ny))=normal(a,b,half)else{return Ok(())};let center=if start{Point::new(a.x-ux*half,a.y-uy*half)}else{Point::new(b.x+ux*half,b.y+uy*half)};let edge=if start{a}else{b};add_polygon(out,&[Point::new(center.x+nx,center.y+ny),Point::new(edge.x+nx,edge.y+ny),Point::new(edge.x-nx,edge.y-ny),Point::new(center.x-nx,center.y-ny)])}

struct Scanner<'a>{bytes:&'a [u8],at:usize}
impl<'a> Scanner<'a>{fn new(text:&'a str)->Self{Self{bytes:text.as_bytes(),at:0}}fn skip_sep_at(&self,mut at:usize)->usize{while let Some(byte)=self.bytes.get(at).copied(){if byte.is_ascii_whitespace()||byte==b','{at=at.saturating_add(1)}else{break}}at}fn skip_sep(&mut self){self.at=self.skip_sep_at(self.at)}fn has_more(&mut self)->bool{self.skip_sep();self.at<self.bytes.len()}fn peek_command(&mut self)->Option<u8>{self.skip_sep();self.bytes.get(self.at).copied().filter(u8::is_ascii_alphabetic)}fn command(&mut self)->Result<u8>{self.skip_sep();let command=self.bytes.get(self.at).copied().ok_or_else(||Error::damaged("expected SVG command"))?;if !command.is_ascii_alphabetic(){return Err(Error::damaged("expected SVG command letter"));}self.at=self.at.saturating_add(1);Ok(command)}fn has_number(&self)->bool{let at=self.skip_sep_at(self.at);matches!(self.bytes.get(at).copied(),Some(b'+')|Some(b'-')|Some(b'.')|Some(b'0'..=b'9'))}fn point(&mut self)->Result<Point>{Ok(Point::new(self.number()?,self.number()?))}fn flag(&mut self)->Result<bool>{let value=self.number()?;if value==0.0{Ok(false)}else if value==1.0{Ok(true)}else{Err(Error::damaged("SVG arc flag must be 0 or 1"))}}fn number(&mut self)->Result<f32>{self.skip_sep();let start=self.at;let mut saw_digit=false;if matches!(self.bytes.get(self.at),Some(b'+')|Some(b'-')){self.at=self.at.saturating_add(1);}while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){saw_digit=true;self.at=self.at.saturating_add(1);}if self.bytes.get(self.at)==Some(&b'.'){self.at=self.at.saturating_add(1);while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){saw_digit=true;self.at=self.at.saturating_add(1);}}if !saw_digit{return Err(Error::damaged("invalid SVG number"));}if matches!(self.bytes.get(self.at),Some(b'e')|Some(b'E')){self.at=self.at.saturating_add(1);if matches!(self.bytes.get(self.at),Some(b'+')|Some(b'-')){self.at=self.at.saturating_add(1);}let exponent_start=self.at;while self.bytes.get(self.at).is_some_and(u8::is_ascii_digit){self.at=self.at.saturating_add(1);}if exponent_start==self.at{return Err(Error::damaged("invalid SVG exponent"));}}let bytes=self.bytes.get(start..self.at).ok_or_else(||Error::damaged("SVG number range invalid"))?;let text=std::str::from_utf8(bytes).map_err(|error|Error::damaged(error.to_string()))?;let value=text.parse::<f32>().map_err(|_|Error::damaged("invalid SVG floating point value"))?;if !value.is_finite(){return Err(Error::damaged("non-finite SVG number"));}Ok(value)}}

/// Built-in interface icon identifier.
#[derive(Debug,Clone,Copy,PartialEq,Eq,Hash)]
pub enum Icon{/// Save collection.
Saves,/// Inventory backpack.
Inventory,/// Stash box.
Stash,/// Map/transitions.
MapTransitions,/// Factions.
Factions,/// Backup.
Backup,/// Compare.
Compare,/// Timeline.
Timeline,/// Game doctor.
Doctor,/// Games.
Games,/// Fixes.
Fixes,/// Wrench.
Wrench,/// Companion.
Companion,/// Trophy.
Trophy,/// Cloud.
Cloud,/// Book.
Book,/// Shield/capabilities.
ShieldCapabilities,/// Update.
Update,/// Settings.
Settings,/// Search.
Search,/// Add.
Add,/// Delete.
Delete,/// Undo.
Undo,/// Redo.
Redo,/// Save action.
Save,/// Folder.
Folder,/// Warning.
Warning,/// Information.
Info}

/// Returns the hand-authored 24×24 SVG path for an icon. Paths are designed for a 2 px round stroke.
#[must_use]
pub const fn icon_path(icon:Icon)->&'static str{match icon{
Icon::Saves=>"M5 3 L17 3 L21 7 L21 21 L3 21 L3 3 Z M7 3 L7 9 L16 9 L16 3 M7 15 L17 15 M7 18 L15 18",
Icon::Inventory=>"M7 7 C7 4 17 4 17 7 L20 20 L4 20 Z M8 10 L8 13 M16 10 L16 13 M9 7 L9 5 M15 7 L15 5",
Icon::Stash=>"M3 7 L21 7 L19 20 L5 20 Z M2 4 L22 4 L22 7 L2 7 Z M9 11 L15 11",
Icon::MapTransitions=>"M4 5 L9 3 L15 5 L20 3 L20 19 L15 21 L9 19 L4 21 Z M9 3 L9 19 M15 5 L15 21 M6 12 L12 12 M10 9 L13 12 L10 15",
Icon::Factions=>"M12 3 L16 8 L21 9 L17 13 L18 19 L12 17 L6 19 L7 13 L3 9 L8 8 Z M12 7 L12 14 M9 11 L15 11",
Icon::Backup=>"M12 4 A8 8 0 1 1 5 8 M5 4 L5 8 L9 8 M12 8 L12 13 L16 15",
Icon::Compare=>"M7 4 L3 8 L7 12 M3 8 L15 8 M17 12 L21 16 L17 20 M21 16 L9 16",
Icon::Timeline=>"M4 6 L20 6 M4 12 L20 12 M4 18 L20 18 M8 6 A2 2 0 1 0 8.1 6 M15 12 A2 2 0 1 0 15.1 12 M10 18 A2 2 0 1 0 10.1 18",
Icon::Doctor=>"M9 3 L15 3 L15 8 L20 8 L20 14 L15 14 L15 21 L9 21 L9 14 L4 14 L4 8 L9 8 Z",
Icon::Games=>"M7 8 L17 8 C20 8 22 15 20 18 C19 20 16 16 15 15 L9 15 C8 16 5 20 4 18 C2 15 4 8 7 8 Z M8 11 L8 14 M6.5 12.5 L9.5 12.5 M16 11.5 L16.1 11.5 M18 13.5 L18.1 13.5",
Icon::Fixes=>"M12 3 L14 8 L19 5 L16 10 L21 12 L16 14 L19 19 L14 16 L12 21 L10 16 L5 19 L8 14 L3 12 L8 10 L5 5 L10 8 Z",
Icon::Wrench=>"M14 4 C16 2 19 3 20 4 L17 7 L17 10 L20 13 C18 15 15 15 13 13 L6 20 L3 17 L10 10 C8 7 10 4 14 4 Z",
Icon::Companion=>"M8 9 A4 4 0 1 0 8.1 9 M16 8 A3 3 0 1 0 16.1 8 M3 20 C3 15 13 15 13 20 M13 19 C14 15 21 16 21 20",
Icon::Trophy=>"M7 4 L17 4 L16 11 C16 15 8 15 8 11 Z M7 6 L4 6 C4 10 5 12 8 12 M17 6 L20 6 C20 10 19 12 16 12 M12 15 L12 19 M8 21 L16 21",
Icon::Cloud=>"M7 19 L18 19 C22 19 23 13 19 11 C19 6 12 4 9 8 C5 7 2 10 3 14 C3 17 5 19 7 19 Z",
Icon::Book=>"M4 4 C8 3 10 4 12 6 C14 4 16 3 20 4 L20 20 C16 19 14 20 12 21 C10 20 8 19 4 20 Z M12 6 L12 21",
Icon::ShieldCapabilities=>"M12 3 L20 6 L19 13 C18 18 15 20 12 21 C9 20 6 18 5 13 L4 6 Z M9 12 L11 14 L16 9",
Icon::Update=>"M19 8 L19 3 L14 3 M19 3 L15 7 M5 16 L5 21 L10 21 M5 21 L9 17 M6 8 C8 4 14 3 18 7 M18 16 C16 20 10 21 6 17",
Icon::Settings=>"M12 8 A4 4 0 1 0 12.1 8 M12 3 L14 5 L17 4 L18 7 L21 8 L20 12 L21 15 L18 17 L17 20 L13 19 L10 21 L8 18 L5 18 L4 14 L2 12 L4 9 L4 6 L8 5 Z",
Icon::Search=>"M10 4 A6 6 0 1 0 10.1 4 M15 15 L21 21",
Icon::Add=>"M12 4 L12 20 M4 12 L20 12",
Icon::Delete=>"M5 7 L19 7 M9 7 L9 4 L15 4 L15 7 M7 7 L8 20 L16 20 L17 7 M10 10 L10 17 M14 10 L14 17",
Icon::Undo=>"M9 6 L4 11 L9 16 M5 11 L14 11 C18 11 20 14 20 18",
Icon::Redo=>"M15 6 L20 11 L15 16 M19 11 L10 11 C6 11 4 14 4 18",
Icon::Save=>"M5 3 L17 3 L21 7 L21 21 L3 21 L3 3 Z M7 3 L7 9 L16 9 L16 3 M7 15 L17 15 L17 21 L7 21 Z",
Icon::Folder=>"M3 6 L10 6 L12 8 L21 8 L20 20 L4 20 Z M3 6 L4 4 L9 4 L11 6",
Icon::Warning=>"M12 3 L22 20 L2 20 Z M12 8 L12 14 M12 17 L12.1 17",
Icon::Info=>"M12 3 A9 9 0 1 0 12.1 3 M12 10 L12 17 M12 7 L12.1 7"}}

#[cfg(test)]
mod tests{use super::*;
#[test]fn parser_handles_all_commands_and_implicit_repeats(){let data="M1 2 3 4 h2 v3 l1 1 C7 8 9 10 11 12 s2 3 4 5 Q18 19 20 21 t2 2 A3 4 30 0 1 25 26 z";let path=parse_svg_path(data);assert!(path.is_ok());let p=path.unwrap_or_else(|e|panic!("{e}"));assert!(p.verbs().len()>=10);assert!(matches!(p.verbs().last(),Some(Verb::Close)));}
#[test]fn relative_commands_track_current_point(){let p=parse_svg_path("m10 10 5 0 -2 3").unwrap_or_else(|e|panic!("{e}"));assert_eq!(p.verbs().first(),Some(&Verb::Move(Point::new(10.0,10.0))));assert_eq!(p.verbs().get(1),Some(&Verb::Line(Point::new(15.0,10.0))));assert_eq!(p.verbs().get(2),Some(&Verb::Line(Point::new(13.0,13.0))));}
#[test]fn arc_conversion_hits_exact_endpoint(){let p=parse_svg_path("M0 0 A10 10 0 0 1 10 10").unwrap_or_else(|e|panic!("{e}"));let Some(Verb::Cubic{to,..})=p.verbs().last()else{panic!("arc was not converted to cubic")};assert!((to.x-10.0).abs()<0.0001);assert!((to.y-10.0).abs()<0.0001);let flat=p.flatten(0.1).unwrap_or_else(|e|panic!("{e}"));assert!(!flat.is_empty());}
#[test]fn every_icon_parses(){let icons=[Icon::Saves,Icon::Inventory,Icon::Stash,Icon::MapTransitions,Icon::Factions,Icon::Backup,Icon::Compare,Icon::Timeline,Icon::Doctor,Icon::Games,Icon::Fixes,Icon::Wrench,Icon::Companion,Icon::Trophy,Icon::Cloud,Icon::Book,Icon::ShieldCapabilities,Icon::Update,Icon::Settings,Icon::Search,Icon::Add,Icon::Delete,Icon::Undo,Icon::Redo,Icon::Save,Icon::Folder,Icon::Warning,Icon::Info];for icon in icons{assert!(parse_svg_path(icon_path(icon)).is_ok(),"{icon:?}");}}
#[test]fn stroke_expansion_emits_boundaries(){let p=parse_svg_path("M2 12 L12 4 L22 12").unwrap_or_else(|e|panic!("{e}"));let stroke=stroke_to_fill(&p,StrokeStyle{width:2.0,cap:LineCap::Round,join:LineJoin::Round,miter_limit:4.0},0.1).unwrap_or_else(|e|panic!("{e}"));assert_eq!(stroke.rule,FillRule::NonZero);assert!(stroke.segments.len()>20);}}
