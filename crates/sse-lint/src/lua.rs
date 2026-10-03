//! Lua 5.1 parser for X-Ray scripts.
//!
//! Parsing is bounded to 200 nested constructs and stores syntax in one node arena. Source spans always refer to
//! the original byte buffer, so Windows-1251 strings never need to be decoded to build or inspect the AST.

use crate::lexer::{LuaLexer, Token, TokenKind};
use sse_core::{Error, Result};

const MAX_DEPTH: u16 = 200;
const MAX_NODES: usize = 1_000_000;

/// Arena node identifier.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NodeId(u32);
/// Source byte span and one-based line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span { /// Start byte.
    pub start:usize, /// End byte.
    pub end:usize, /// Source line.
    pub line:usize }
/// Lua AST node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NodeKind {
    /// Statement block.
    Block(Vec<NodeId>), /// Identifier.
    Name, /// Literal.
    Literal, /// Vararg.
    Vararg, /// Unary expression.
    Unary { /// Operator source span.
        op:Span, /// Operand.
        value:NodeId }, /// Binary expression.
    Binary { /// Operator source span.
        op:Span, /// Left operand.
        left:NodeId, /// Right operand.
        right:NodeId }, /// Table constructor.
    Table(Vec<NodeId>), /// Table field.
    Field { /// Optional key; `None` is positional.
        key:Option<NodeId>, /// Value.
        value:NodeId }, /// Field/index expression.
    Index { /// Base.
        base:NodeId, /// Key/name node.
        key:NodeId }, /// Call or method call.
    Call { /// Callee/receiver.
        callee:NodeId, /// Optional method-name node.
        method:Option<NodeId>, /// Arguments.
        args:Vec<NodeId> }, /// Function body.
    Function { /// Parameters.
        params:Vec<NodeId>, /// Has `...` parameter.
        vararg:bool, /// Body.
        body:NodeId }, /// Assignment/local declaration.
    Assign { /// Left/name nodes.
        left:Vec<NodeId>, /// Values.
        right:Vec<NodeId>, /// Local declaration.
        local:bool }, /// Function declaration.
    FunctionDecl { /// Target/name.
        target:NodeId, /// Function node.
        function:NodeId, /// Local declaration.
        local:bool }, /// Return.
    Return(Vec<NodeId>), /// Break.
    Break, /// `do`.
    Do(NodeId), /// `while`.
    While { /// Condition.
        condition:NodeId, /// Body.
        body:NodeId }, /// `repeat`.
    Repeat { /// Body.
        body:NodeId, /// Condition.
        condition:NodeId }, /// `if` branches `(condition, body)`, with `None` for else.
    If(Vec<(Option<NodeId>,NodeId)>), /// Numeric `for`.
    ForNumeric { /// Name.
        name:NodeId, /// Initial value.
        initial:NodeId, /// Limit.
        limit:NodeId, /// Optional step.
        step:Option<NodeId>, /// Body.
        body:NodeId }, /// Generic `for`.
    ForGeneric { /// Names.
        names:Vec<NodeId>, /// Iterator expressions.
        values:Vec<NodeId>, /// Body.
        body:NodeId },
}
/// Node plus complete source extent.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node { /// Payload.
    pub kind:NodeKind, /// Source span.
    pub span:Span }
/// Parsed Lua chunk.
#[derive(Debug)]
pub struct Ast<'a> { source:&'a [u8], nodes:Vec<Node>, root:NodeId }
impl<'a> Ast<'a> { /// Root block.
    #[must_use] pub fn root(&self)->NodeId{self.root} /// Arena nodes.
    #[must_use] pub fn nodes(&self)->&[Node]{&self.nodes} /// Gets one node.
    #[must_use] pub fn node(&self,id:NodeId)->Option<&Node>{self.nodes.get(usize::try_from(id.0).unwrap_or(usize::MAX))} /// Raw bytes for a span.
    #[must_use] pub fn bytes(&self,span:Span)->&[u8]{self.source.get(span.start..span.end).unwrap_or_default()} }

struct Parser<'a>{source:&'a [u8],tokens:Vec<Token>,at:usize,nodes:Vec<Node>,depth:u16}
impl<'a> Parser<'a>{
 fn new(source:&'a[u8])->Self{let mut lexer=LuaLexer::new(source);Self{source,tokens:lexer.tokenize_all(),at:0,nodes:Vec::new(),depth:0}}
 fn tok(&self)->&Token{self.tokens.get(self.at).or_else(||self.tokens.last()).unwrap_or(&Token{kind:TokenKind::Eof,line:1,column:1,start:0,end:0})}
 fn span(t:&Token)->Span{Span{start:t.start,end:t.end,line:t.line}}
 fn text(&self)->&[u8]{let t=self.tok();self.source.get(t.start..t.end).unwrap_or_default()}
 fn bump(&mut self)->Token{let t=self.tok().clone();self.at=self.at.saturating_add(1);t}
 fn sym(&self,s:&str)->bool{matches!(&self.tok().kind,TokenKind::Symbol(v) if v==s)} fn kw(&self,s:&str)->bool{matches!(&self.tok().kind,TokenKind::Keyword(v) if v==s)}
 fn eat_sym(&mut self,s:&str)->bool{if self.sym(s){let _=self.bump();true}else{false}} fn eat_kw(&mut self,s:&str)->bool{if self.kw(s){let _=self.bump();true}else{false}}
 fn err(&self,msg:&str)->Error{let t=self.tok();Error::damaged(format!("lua:{}:{}: {msg}",t.line,t.column))}
 fn need_sym(&mut self,s:&str)->Result<Token>{if self.sym(s){Ok(self.bump())}else{Err(self.err("symbol expected"))}} fn need_kw(&mut self,s:&str)->Result<Token>{if self.kw(s){Ok(self.bump())}else{Err(self.err("keyword expected"))}}
 fn alloc(&mut self,kind:NodeKind,span:Span)->Result<NodeId>{if self.nodes.len()>=MAX_NODES{return Err(Error::Refused("Lua AST node limit exceeded".to_owned()));}let id=u32::try_from(self.nodes.len()).map_err(|_|Error::Refused("Lua AST too large".to_owned()))?;self.nodes.push(Node{kind,span});Ok(NodeId(id))}
 fn node_span(&self,id:NodeId)->Span{self.nodes.get(usize::try_from(id.0).unwrap_or(usize::MAX)).map_or(Span{start:0,end:0,line:1},|n|n.span)}
 fn enter(&mut self)->Result<()> {self.depth=self.depth.checked_add(1).ok_or_else(||self.err("nesting overflow"))?;if self.depth>MAX_DEPTH{return Err(self.err("more than 200 nested constructs"));}Ok(())} fn leave(&mut self){self.depth=self.depth.saturating_sub(1);}
 fn name(&mut self)->Result<NodeId>{if !matches!(self.tok().kind,TokenKind::Identifier(_)){return Err(self.err("name expected"));}let t=self.bump();self.alloc(NodeKind::Name,Self::span(&t))}
 fn block(&mut self,stops:&[&str])->Result<NodeId>{self.enter()?;let start=Self::span(self.tok());let mut out=Vec::new();while !matches!(self.tok().kind,TokenKind::Eof)&&!stops.iter().any(|s|self.kw(s)){out.push(self.statement()?);let _=self.eat_sym(";");}let end=out.last().map_or(start.end,|id|self.node_span(*id).end);let id=self.alloc(NodeKind::Block(out),Span{start:start.start,end,line:start.line});self.leave();id}
 fn statement(&mut self)->Result<NodeId>{
  if self.eat_kw("break"){let t=self.tokens.get(self.at.saturating_sub(1)).cloned().unwrap_or_else(||self.tok().clone());return self.alloc(NodeKind::Break,Self::span(&t));}
  if self.eat_kw("return"){let start=Self::span(self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()));let values=if self.is_block_end()||self.sym(";"){Vec::new()}else{self.expr_list()?};let end=values.last().map_or(start.end,|id|self.node_span(*id).end);return self.alloc(NodeKind::Return(values),Span{start:start.start,end,line:start.line});}
  if self.eat_kw("do"){let start=Self::span(self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()));let body=self.block(&["end"])?;let end=self.need_kw("end")?;return self.alloc(NodeKind::Do(body),Span{start:start.start,end:end.end,line:start.line});}
  if self.eat_kw("while"){let start=Self::span(self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()));let condition=self.expr(0)?;self.need_kw("do")?;let body=self.block(&["end"])?;let end=self.need_kw("end")?;return self.alloc(NodeKind::While{condition,body},Span{start:start.start,end:end.end,line:start.line});}
  if self.eat_kw("repeat"){let start=Self::span(self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()));let body=self.block(&["until"])?;self.need_kw("until")?;let condition=self.expr(0)?;let end=self.node_span(condition).end;return self.alloc(NodeKind::Repeat{body,condition},Span{start:start.start,end,line:start.line});}
  if self.eat_kw("if"){return self.if_stmt();} if self.eat_kw("for"){return self.for_stmt();} if self.eat_kw("function"){return self.function_decl(false);} if self.eat_kw("local"){if self.eat_kw("function"){return self.function_decl(true);}return self.local_stmt();}
  let first=self.prefix()?;if self.sym("=")||self.sym(","){let mut left=vec![first];while self.eat_sym(","){left.push(self.prefix()?);}self.need_sym("=")?;let right=self.expr_list()?;let start=self.node_span(first);let end=right.last().map_or(start.end,|id|self.node_span(*id).end);return self.alloc(NodeKind::Assign{left,right,local:false},Span{start:start.start,end,line:start.line});}if matches!(self.nodes.get(usize::try_from(first.0).unwrap_or(usize::MAX)).map(|n|&n.kind),Some(NodeKind::Call{..})){return Ok(first);}Err(self.err("assignment or function call expected")) }
 fn is_block_end(&self)->bool{matches!(&self.tok().kind,TokenKind::Eof)||["end","else","elseif","until"].iter().any(|s|self.kw(s))}
 fn if_stmt(&mut self)->Result<NodeId>{let start=Self::span(self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()));let mut branches=Vec::new();let cond=self.expr(0)?;self.need_kw("then")?;branches.push((Some(cond),self.block(&["elseif","else","end"])?));while self.eat_kw("elseif"){let c=self.expr(0)?;self.need_kw("then")?;branches.push((Some(c),self.block(&["elseif","else","end"])?));}if self.eat_kw("else"){branches.push((None,self.block(&["end"])?));}let end=self.need_kw("end")?;self.alloc(NodeKind::If(branches),Span{start:start.start,end:end.end,line:start.line})}
 fn for_stmt(&mut self)->Result<NodeId>{let start=Self::span(self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()));let name=self.name()?;if self.eat_sym("="){let initial=self.expr(0)?;self.need_sym(",")?;let limit=self.expr(0)?;let step=if self.eat_sym(","){Some(self.expr(0)?)}else{None};self.need_kw("do")?;let body=self.block(&["end"])?;let end=self.need_kw("end")?;return self.alloc(NodeKind::ForNumeric{name,initial,limit,step,body},Span{start:start.start,end:end.end,line:start.line});}let mut names=vec![name];while self.eat_sym(","){names.push(self.name()?);}self.need_kw("in")?;let values=self.expr_list()?;self.need_kw("do")?;let body=self.block(&["end"])?;let end=self.need_kw("end")?;self.alloc(NodeKind::ForGeneric{names,values,body},Span{start:start.start,end:end.end,line:start.line})}
 fn function_decl(&mut self,local:bool)->Result<NodeId>{let start=Self::span(self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()));let mut target=self.name()?;if !local{while self.eat_sym("."){let key=self.name()?;let span=self.node_span(target);target=self.alloc(NodeKind::Index{base:target,key},Span{start:span.start,end:self.node_span(key).end,line:span.line})?;}if self.eat_sym(":"){let key=self.name()?;let span=self.node_span(target);target=self.alloc(NodeKind::Index{base:target,key},Span{start:span.start,end:self.node_span(key).end,line:span.line})?;}}let function=self.function_body()?;let end=self.node_span(function).end;self.alloc(NodeKind::FunctionDecl{target,function,local},Span{start:start.start,end,line:start.line})}
 fn local_stmt(&mut self)->Result<NodeId>{let first=self.name()?;let start=self.node_span(first);let mut left=vec![first];while self.eat_sym(","){left.push(self.name()?);}let right=if self.eat_sym("="){self.expr_list()?}else{Vec::new()};let end=right.last().or_else(||left.last()).map_or(start.end,|id|self.node_span(*id).end);self.alloc(NodeKind::Assign{left,right,local:true},Span{start:start.start,end,line:start.line})}
 fn function_body(&mut self)->Result<NodeId>{let open=self.need_sym("(")?;self.enter()?;let mut params=Vec::new();let mut vararg=false;if !self.sym(")"){if self.eat_sym("..."){vararg=true;}else{params.push(self.name()?);while self.eat_sym(","){if self.eat_sym("..."){vararg=true;break;}params.push(self.name()?);}}}self.need_sym(")")?;let body=self.block(&["end"])?;let end=self.need_kw("end")?;let id=self.alloc(NodeKind::Function{params,vararg,body},Span{start:open.start,end:end.end,line:open.line});self.leave();id}
 fn expr_list(&mut self)->Result<Vec<NodeId>>{let mut v=vec![self.expr(0)?];while self.eat_sym(","){v.push(self.expr(0)?);}Ok(v)}
 fn expr(&mut self,min:u8)->Result<NodeId>{let mut left=self.unary()?;loop{let Some((lb,rb))=self.bin_bp()else{break;};if lb<min{break;}let op=self.bump();let right=self.expr(rb)?;let span=self.node_span(left);left=self.alloc(NodeKind::Binary{op:Self::span(&op),left,right},Span{start:span.start,end:self.node_span(right).end,line:span.line})?;}Ok(left)}
 fn bin_bp(&self)->Option<(u8,u8)>{let t=self.text();match t{b"or"=>Some((1,2)),b"and"=>Some((2,3)),b"<"|b">"|b"<="|b">="|b"~="|b"=="=>Some((3,4)),b".."=>Some((4,4)),b"+"|b"-"=>Some((5,6)),b"*"|b"/"|b"%"=>Some((6,7)),b"^"=>Some((8,8)),_=>None}}
 fn unary(&mut self)->Result<NodeId>{if matches!(self.text(),b"not"|b"-"|b"#"){let op=self.bump();let value=self.expr(7)?;return self.alloc(NodeKind::Unary{op:Self::span(&op),value},Span{start:op.start,end:self.node_span(value).end,line:op.line});}self.simple()}
 fn simple(&mut self)->Result<NodeId>{if matches!(self.tok().kind,TokenKind::NumberLiteral(_)|TokenKind::StringLiteral(_))||matches!(self.text(),b"nil"|b"true"|b"false"){let t=self.bump();return self.alloc(NodeKind::Literal,Self::span(&t));}if self.eat_sym("..."){let t=self.tokens.get(self.at.saturating_sub(1)).unwrap_or(self.tok()).clone();return self.alloc(NodeKind::Vararg,Self::span(&t));}if self.eat_kw("function"){return self.function_body();}if self.sym("{"){return self.table();}self.prefix()}
 fn prefix(&mut self)->Result<NodeId>{let mut node=if matches!(self.tok().kind,TokenKind::Identifier(_)){self.name()?}else if self.eat_sym("("){let value=self.expr(0)?;self.need_sym(")")?;value}else{return Err(self.err("expression expected"));};loop{if self.eat_sym("["){let key=self.expr(0)?;let close=self.need_sym("]")?;let s=self.node_span(node);node=self.alloc(NodeKind::Index{base:node,key},Span{start:s.start,end:close.end,line:s.line})?;}else if self.eat_sym("."){let key=self.name()?;let s=self.node_span(node);node=self.alloc(NodeKind::Index{base:node,key},Span{start:s.start,end:self.node_span(key).end,line:s.line})?;}else if self.eat_sym(":"){let method=self.name()?;let args=self.args()?;let s=self.node_span(node);let end=args.last().map_or(self.node_span(method).end,|id|self.node_span(*id).end);node=self.alloc(NodeKind::Call{callee:node,method:Some(method),args},Span{start:s.start,end,line:s.line})?;}else if self.starts_args(){let args=self.args()?;let s=self.node_span(node);let end=args.last().map_or(s.end,|id|self.node_span(*id).end);node=self.alloc(NodeKind::Call{callee:node,method:None,args},Span{start:s.start,end,line:s.line})?;}else{break;}}Ok(node)}
 fn starts_args(&self)->bool{self.sym("(")||self.sym("{")||matches!(self.tok().kind,TokenKind::StringLiteral(_))}
 fn args(&mut self)->Result<Vec<NodeId>>{if self.eat_sym("("){let v=if self.sym(")"){Vec::new()}else{self.expr_list()?};self.need_sym(")")?;return Ok(v);}if self.sym("{"){return Ok(vec![self.table()?]);}if matches!(self.tok().kind,TokenKind::StringLiteral(_)){let t=self.bump();return Ok(vec![self.alloc(NodeKind::Literal,Self::span(&t))?]);}Err(self.err("arguments expected"))}
 fn table(&mut self)->Result<NodeId>{let open=self.need_sym("{")?;self.enter()?;let mut fields=Vec::new();while !self.sym("}"){let start=Self::span(self.tok());let (key,value)=if self.eat_sym("["){let k=self.expr(0)?;self.need_sym("]")?;self.need_sym("=")?;(Some(k),self.expr(0)?)}else if matches!(self.tok().kind,TokenKind::Identifier(_))&&self.tokens.get(self.at.saturating_add(1)).is_some_and(|t|matches!(&t.kind,TokenKind::Symbol(v) if v=="=")){let k=self.name()?;self.need_sym("=")?;(Some(k),self.expr(0)?)}else{(None,self.expr(0)?)};let end=self.node_span(value).end;fields.push(self.alloc(NodeKind::Field{key,value},Span{start:start.start,end,line:start.line})?);if !self.eat_sym(",")&&!self.eat_sym(";"){break;}}let close=self.need_sym("}")?;let id=self.alloc(NodeKind::Table(fields),Span{start:open.start,end:close.end,line:open.line});self.leave();id}
}
/// Parses a complete Lua 5.1 chunk.
///
/// # Errors
/// Reports syntax errors with `lua:line:column` and refuses nesting deeper than 200 constructs.
pub fn parse(source:&[u8])->Result<Ast<'_>>{let mut p=Parser::new(source);let root=p.block(&[])?;if !matches!(p.tok().kind,TokenKind::Eof){return Err(p.err("unexpected token"));}Ok(Ast{source,nodes:p.nodes,root})}

#[cfg(test)] mod tests {use super::*;#[test]fn language_samples(){let samples:[&[u8];24]=[b"",b"a=1",b"local a,b=1,2",b"function a:b(x,...) return x end",b"if a then b() elseif c then d=2 else e=3 end",b"while a do break end",b"repeat a=a+1 until a>2",b"for i=1,10,2 do f(i) end",b"for k,v in pairs(t) do f(k,v) end",b"a={1,x=2,[k]=3}",b"a=2^3^4",b"a='x'..'y'",b"a=not b and c or d",b"o:m(1)",b"f'x'",b"f{}",b"a=[=[long]=]",b"--[==[c]==]\na=1",b"a=0xff",b"a=.5",b"a=1e-3",b"do local x=1 end",b"return function() return 1 end",b"x=t[a].b"];for s in samples{assert!(parse(s).is_ok(),"{}",String::from_utf8_lossy(s));}}#[test]fn cp1251_string_bytes_are_accepted(){assert!(parse(b"x='\xcf\xf0\xe8\xef\xff\xf2\xfc'").is_ok());}#[test]fn every_truncation_is_safe(){let script=b"function f(a) if a then return {x=1,[a]=2} else return g(a) end end";for n in 0..script.len(){assert!(std::panic::catch_unwind(||parse(script.get(..n).unwrap_or_default())).is_ok());}}#[test]fn nesting_limit(){let mut s=Vec::new();for _ in 0..201{s.extend_from_slice(b"do ");}for _ in 0..201{s.extend_from_slice(b"end ");}assert!(parse(&s).is_err());}}
