#![allow(dead_code)]
// TODO: Figure out how to handle functions which are in Symbols table.
use lexer::{Lexer, Token};
use symbols::{SYMBOLS, Symbol, IsSymbol, FontMode};
use parser::nodes::{ AtomType, Delimited, ParseNode, Scripts };

use functions::COMMANDS;

/// This method is served as an entry point to parsing the input.
/// It can also but used to parse sub-expressions (or more formally known)
/// as `mathlists` which can be found from parsing groups.
///
/// This method will always return something, though it may be an emptylist.
/// This method itself will not fail, but it is possible that expressions
/// inside this method will fail and raise and error. 

fn expression(lex: &mut Lexer) -> Result<Vec<ParseNode>, String> {
    let mut ml: Vec<ParseNode> = Vec::new();

    loop {
        // TODO: We need to check parsing mode here for properly handling spaces.
        // TODO: Handle INFIX operators here.
        lex.consume_whitespace();
        if lex.current.ends_expression() { break; }

        let mut node = first_some!(lex, command, group, symbol, implicit_group,);

        // Here we handle all post-fix operators, like superscripts, subscripts
        // `\limits`, `\nolimits`, and anything that may require us to modify
        // the current vector of ParseNodes
        loop {
            lex.consume_whitespace();
            match lex.current {
                Token::Symbol('_') => {
                    lex.next();
                    let script = math_field(lex)?;

                    if let Some(ParseNode::Scripts(ref mut b)) = node {
                        // We are already parsing a script, place the next
                        // group into the appropriate field.  If we already
                        // have a subscript, this is an error.
                        if b.subscript.is_some() {
                            return Err("Multiple subscripts!".to_string());
                        }
                        b.subscript = Some(Box::new(script));
                    } else {
                        // This is our first script, so we need to create a 
                        // new one.
                        node = Some(ParseNode::Scripts(Scripts {
                            base:
                                match node {
                                    None => None,
                                    Some(n) => Some(Box::new(n))
                                },
                            subscript: Some(Box::new(script)),
                            superscript: None,
                        }));
                    }
                },
                Token::Symbol('^') => {
                    lex.next();
                    let script = math_field(lex)?;
                    if let Some(ParseNode::Scripts(ref mut b)) = node {
                        if b.superscript.is_some() {
                            return Err("Multiple superscripts!".to_string());
                        }
                        b.superscript = Some(Box::new(script));
                    } else {
                        node = Some(ParseNode::Scripts(Scripts {
                            base: 
                                match node {
                                    None => None,
                                    Some(n) => Some(Box::new(n))
                                },
                            superscript: Some(Box::new(script)),
                            subscript: None,
                        }));
                    }
                },
                _ => { break; }
            }
            // End of post-fix processing
        }

        ml.push(match node {
            None => return Err(format!("Unable to parse {:?}", node)),
            Some(s) => s
        });
    }

    Ok(ml)
}

/// Parse a `<Math Field>`.  A math field is defined by
///
/// ```bnf,ignore
/// <Math_Field> = <filler><Symbol> | <filler>{<mathmode material>}
/// ```
///
/// See page 289 of the TeX book for more details.
/// This method will result in an error if either the `Symbol` or
/// `<mathmode material>` contains an error, or if no match is found.

pub fn math_field(lex: &mut Lexer) -> Result<ParseNode, String> {
    first_some!(lex, command, group, symbol,)
        .ok_or(format!("Expected a mathfield following: {:?}", lex.current))
}

/// Parse a TeX command. These commands define the "primitive" commands for our
/// typesetting system.  It (should) include a large portion of the TeX primitives,
/// along with the most useful primitives you find from amsmath and LaTeX.
/// If no matching command is found, this will return `Ok(None)`.  This method
/// can fail while parsing parameters for a TeX command.

pub fn command(lex: &mut Lexer) -> Result<Option<ParseNode>, String> {
    // TODO: We need to build a framework, that will match commands 
    let cmd = if let Token::ControlSequence(cmd) = lex.current {
        match COMMANDS.get(cmd).cloned() {
            Some(command) => command,
            None => return Ok(None),
        }
    } else {
        return Ok(None)
    };

    // A command has been found.  Consume the token and parse for arguments. 
    lex.next();
    cmd.parse(lex)
}

/// Parse an implicit group.  An implicit group is often defined by a command
/// that implicitly has a `\bgroup` or `{` somewhere inside it's definition.  This is one
/// point where we will deviate from TeX a little bit.  We won't characterize every
/// command that will start a new implicit group (for instance, `\frac`).
///
/// This should be used almost anywhere `group()` is used.

pub fn implicit_group(lex: &mut Lexer) -> Result<Option<ParseNode>, String> {
    let token = lex.current;

    if token == Token::ControlSequence("left") {
        lex.next(); // consume the `\left` token`
        let left  = expect_type(lex, AtomType::Open)?;
        let inner = expression(lex)?;
        lex.current.expect(Token::ControlSequence("right"))?;
        lex.next();
        let right = expect_type(lex, AtomType::Close)?;

        Ok(Some(ParseNode::Delimited(Delimited{
            left: left,
            right: right,
            inner: inner,
        })))
    } else {
        Ok(None)
    }
}

/// Parse a group.  Which is defined by `{<mathmode material>}`.
/// This function will return `Ok(None)` if it does not find a `{`,
/// and will `Err` if it finds a `{` with no terminating `}`, or if
/// there is a syntax error from within `<mathmode material>`.

// TODO: This should also recognize `\bgroup` if we decide to go that route.

pub fn group(lex: &mut Lexer) -> Result<Option<ParseNode>, String> {
    if lex.current == Token::Symbol('{') {
        lex.next();
        let inner = expression(lex)?;
        lex.current.expect(Token::Symbol('}'))?;
        lex.next();
        Ok(Some(ParseNode::Group(inner)))
    } else {
        Ok(None)
    }
}

/// Parse a symbol.  Symbols can be found from a TeX command (like `\infty`)
/// or from a unicode character input.  This function will return `Ok(None)`
/// if the current token is a TeX command which is not found in the symbols
/// table. If there is no defined representation for the given `Token::Symbol`
/// then this function will return with an error.
///
/// Note, there are some `char` inputs that no work here.  For instance,
/// the `{` will not be recognized here and will therefore result in an `Err`.
/// So in general, you should always parse for a group before parsing for a symbol.

pub fn symbol(lex: &mut Lexer) -> Result<Option<ParseNode>, String> {
    match lex.current {
        Token::ControlSequence(cs) => {
            match SYMBOLS.get(cs).cloned() {
                None => Ok(None),
                Some(sym) => { lex.next(); Ok(Some(ParseNode::Symbol(sym))) },
            }
        },
        Token::Symbol(c) => {
            // TODO: Properly handle fontmode here.
            match c.atom_type(FontMode::Italic) {
                //None => Err(format!("Unable to find symbol representation for {}", c)),
                None => Ok(None),
                Some(sym) => { lex.next(); Ok(Some(ParseNode::Symbol(sym))) },
            }
        },
        _ => Ok(None),
    }
}

/// This method expects to parse a single macro argument.  Whitespace will not be consumed
/// while parsing this argument, unless the argument is a command.
/// A macro argument will consume a single token, unless there is a group found { }.
/// In which case, a macro_argument will strip the surrounding { }.  Because of this,
/// the result may be either a single ParseNode, or a vector of ParseNodes.
///
/// Open questions:
///   - How to properly inline a vector of parsenodes?
///   - When can this possible fail?
///   - How to handle custom validators/parsers for arguments. ie: Argument is a color.

pub fn macro_argument(lex: &mut Lexer) -> Result<Option<Vec<ParseNode>>, String> {
    // Must figure out how to properly handle implicit groups here.
    match first_some!(lex, group, symbol,) {
        Some(ParseNode::Symbol(sym)) => Ok(Some(vec![ParseNode::Symbol(sym)])),
        Some(ParseNode::Group(inner)) => Ok(Some(inner)),
        _ => Ok(None),
    }
}

/// This method is like `macro_argument` except that it requires an argument to be present.

pub fn required_macro_argument(lex: &mut Lexer) -> Result<Vec<ParseNode>, String> {
    let arg = macro_argument(lex)?;
    match arg {
        None => Err(format!("Expected a required macro argument! {:?}", arg)),
        Some(res) => Ok(res),
    }
}

/// DOCUMENT ME

#[allow(unused_variables)]
pub fn optional_macro_argument(lex: &mut Lexer) -> Result<Option<Vec<ParseNode>>, String> {
    unimplemented!()
}

/// This method will be used to allow for customized macro argument parsing?

#[allow(unused_variables)]
pub fn special_macro_argument(lex: &mut Lexer) -> () {
    unimplemented!()
}

/// This method expects that the current token has a given atom type.  This method
/// will frist strip all whitespaces first before inspecting the current token.
/// This function will Err if the expected symbol doesn't have the given type,
/// otherwise it will return `Ok`.
///
/// This function _will_ advance the lexer.

pub fn expect_type(lex: &mut Lexer, expected: AtomType) -> Result<Symbol, String> {
    lex.consume_whitespace();

    if let Some(ParseNode::Symbol(sym)) = symbol(lex)? {
        if sym.atom_type == expected {
            Ok(sym)
        } else {
            Err(format!("Expected a symbol of type {:?}, got a symbol of type {:?}",
                expected, sym.atom_type))
        }
    } else {
        Err(format!("Expected a symbol of type {:?}, got a {:?}", expected, lex.current))
    }
}

/// This function is the API entry point for parsing a macro.  For now, it takes a `&str`
/// and outputs a vector of parsing nodes, or an error message.

pub fn parse(input: &str) -> Result<Vec<ParseNode>, String> {
    let mut lexer = Lexer::new(input);
    expression(&mut lexer)
}


// --------------
//     TESTS      
// --------------

#[cfg(test)]
mod tests {
    use parser::nodes::{ ParseNode, AtomType, Radical, Delimited };
    use parser::parse;
    use symbols::Symbol;

    #[test]
    fn parser() {
        assert_eq!(parse(r"").unwrap(), vec![]);
        
        assert_eq!(parse(r" 1 + \sqrt   2").unwrap(), parse(r"1+\sqrt2").unwrap());
        assert_eq!(parse(r"\sqrt  {  \sqrt  2 }").unwrap(), parse(r"\sqrt{\sqrt2}").unwrap());

        assert_eq!(parse(r"1 + {2 + 3}").unwrap(),
            vec![ParseNode::Symbol(Symbol { code: 120803, atom_type: AtomType::Alpha }), 
                ParseNode::Symbol(Symbol { code: 43, atom_type: AtomType::Binary }), 
                ParseNode::Group(vec![ParseNode::Symbol(Symbol { code: 120804, atom_type: AtomType::Alpha }), 
                    ParseNode::Symbol(Symbol { code: 43, atom_type: AtomType::Binary }), 
                    ParseNode::Symbol(Symbol { code: 120805, atom_type: AtomType::Alpha })
            ])]);

        assert_eq!(parse(r"1+\left(3+2\right)=6").unwrap(),
            vec![ParseNode::Symbol(Symbol { code: 120803, atom_type: AtomType::Alpha }), 
                ParseNode::Symbol(Symbol { code: 43, atom_type: AtomType::Binary }), 
                ParseNode::Delimited(Delimited { 
                    left: Symbol { code: 40, atom_type: AtomType::Open }, 
                    right: Symbol { code: 41, atom_type: AtomType::Close }, 
                    inner: vec![ParseNode::Symbol(Symbol { code: 120805, atom_type: AtomType::Alpha }), 
                       ParseNode::Symbol(Symbol { code: 43, atom_type: AtomType::Binary }), 
                       ParseNode::Symbol(Symbol { code: 120804, atom_type: AtomType::Alpha })],
                }), 
                ParseNode::Symbol(Symbol { code: 61, atom_type: AtomType::Relation }), 
                ParseNode::Symbol(Symbol { code: 120808, atom_type: AtomType::Alpha })]);
        
        assert_eq!(parse(r"1+\sqrt2").unwrap(),
            vec![ParseNode::Symbol(Symbol { code: 120803, atom_type: AtomType::Alpha }), 
                 ParseNode::Symbol(Symbol { code: 43, atom_type: AtomType::Binary }), 
                 ParseNode::Radical(Radical { 
                    inner: vec![ParseNode::Symbol(Symbol { code: 120804, atom_type: AtomType::Alpha })] 
                 })]);
    }

    #[test]
    fn fractions() {
        let mut errs: Vec<String> = Vec::new();
        should_pass!(errs, parse,
          [ r"\frac\alpha\beta", r"\frac\int2" ]);
        should_fail!(errs, parse,
          [ r"\frac \left(1 + 2\right) 3" ]);
        should_equate!(errs, parse,
          [ (r"\frac12", r"\frac{1}{2}"), 
            (r"\frac \sqrt2 3", r"\frac{\sqrt2}{3}"),
            (r"\frac \frac 1 2 3", r"\frac{\frac12}{3}"),
            (r"\frac 1 \sqrt2", r"\frac{1}{\sqrt2}") ]);
        display_errors!(errs);
    }

    #[test]
    fn radicals() {
        let mut errs: Vec<String> = Vec::new();
        // TODO: Add optional paramaters for radicals
        should_pass!(errs, parse,
          [ r"\sqrt{x}", r"\sqrt2", r"\sqrt\alpha", r"1^\sqrt2", 
            r"\alpha_\sqrt{1+2}", r"\sqrt\sqrt2" ]);
        should_fail!(errs, parse,
          [ r"\sqrt", r"\sqrt_2", r"\sqrt^2" ]);
        // TODO: Require r"\sqrt2_3" != r"\sqrt{2_3}"
        should_equate!(errs, parse,
          [ (r"\sqrt2", r"\sqrt{2}") ]);
        should_differ!(errs, parse,
          [ (r"\sqrt2_3", r"\sqrt{2_3}") ]);
        display_errors!(errs);
    }

    #[test]
    fn scripts() {
        let mut errs: Vec<String> = Vec::new();
        should_pass!(errs, parse,
          [ r"1_2^3",     
            r"_1", r"^\alpha", r"_2^\alpha",
            r"1_\frac12", r"2^\alpha", 
            r"x_{1+2}", r"x^{2+3}", r"x^{1+2}_{2+3}",
            r"a^{b^c}", r"{a^b}^c", r"a_{b^c}", r"{a_b}^c",
            r"a^{b_c}", r"{a^b}_c", r"a_{b_c}", r"{a_b}_c" ]);
        should_fail!(errs, parse,
          [ r"1_", r"1^", 
            r"x_x_x", r"x^x_x^x", r"x^x^x", r"x_x^x_x" ]);
        should_equate!(errs, parse,
          [ (r"x_\alpha^\beta", r"x^\beta_\alpha"), 
            (r"_2^3", r"^3_2") ]);
        display_errors!(errs);
    }
}