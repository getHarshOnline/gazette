//! Closed file-free DOT profile used before the native Graphviz provider is invoked.

#[derive(Clone, Debug, PartialEq, Eq)]
enum Token {
    Id(String),
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Equal,
    Semicolon,
    Comma,
    Colon,
    DirectedEdge,
    UndirectedEdge,
}

struct Document {
    attributes: Vec<String>,
}

pub(super) fn validate(source: &[u8]) -> Result<(), ()> {
    let source = std::str::from_utf8(source).map_err(|_| ())?;
    let document = Parser::new(tokens(source)?).document()?;
    if document.attributes.iter().all(|name| allowed(name)) {
        Ok(())
    } else {
        Err(())
    }
}

fn allowed(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "arrowhead"
            | "arrowsize"
            | "arrowtail"
            | "bgcolor"
            | "center"
            | "class"
            | "clusterrank"
            | "color"
            | "colorscheme"
            | "compound"
            | "concentrate"
            | "constraint"
            | "decorate"
            | "dir"
            | "distortion"
            | "dpi"
            | "esep"
            | "fillcolor"
            | "fixedsize"
            | "fontcolor"
            | "fontsize"
            | "gradientangle"
            | "group"
            | "headclip"
            | "headlabel"
            | "headport"
            | "height"
            | "href"
            | "id"
            | "label"
            | "labelfloat"
            | "labeljust"
            | "labelloc"
            | "landscape"
            | "len"
            | "lhead"
            | "ltail"
            | "margin"
            | "minlen"
            | "newrank"
            | "nodesep"
            | "normalize"
            | "notranslate"
            | "ordering"
            | "orientation"
            | "outputorder"
            | "overlap"
            | "pack"
            | "packmode"
            | "pad"
            | "penwidth"
            | "peripheries"
            | "pin"
            | "pos"
            | "quantum"
            | "rank"
            | "rankdir"
            | "ranksep"
            | "ratio"
            | "regular"
            | "remincross"
            | "root"
            | "rotate"
            | "samehead"
            | "sametail"
            | "searchsize"
            | "sep"
            | "shape"
            | "showboxes"
            | "sides"
            | "size"
            | "skew"
            | "sortv"
            | "splines"
            | "style"
            | "tailclip"
            | "taillabel"
            | "tailport"
            | "target"
            | "tooltip"
            | "truecolor"
            | "url"
            | "weight"
            | "width"
            | "xlabel"
            | "xdotversion"
    )
}

fn tokens(source: &str) -> Result<Vec<Token>, ()> {
    let mut result = Vec::new();
    let mut input = source.char_indices().peekable();
    while let Some((_, ch)) = input.next() {
        match ch {
            ch if ch.is_whitespace() => {}
            '/' if input.peek().is_some_and(|(_, next)| *next == '/') => {
                input.next();
                for (_, next) in input.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
            }
            '/' if input.peek().is_some_and(|(_, next)| *next == '*') => {
                input.next();
                let mut closed = false;
                let mut previous = '\0';
                for (_, next) in input.by_ref() {
                    if previous == '*' && next == '/' {
                        closed = true;
                        break;
                    }
                    previous = next;
                }
                if !closed {
                    return Err(());
                }
            }
            '{' => result.push(Token::LeftBrace),
            '}' => result.push(Token::RightBrace),
            '[' => result.push(Token::LeftBracket),
            ']' => result.push(Token::RightBracket),
            '=' => result.push(Token::Equal),
            ';' => result.push(Token::Semicolon),
            ',' => result.push(Token::Comma),
            ':' => result.push(Token::Colon),
            '-' => match input.peek().map(|(_, next)| *next) {
                Some('>') => {
                    input.next();
                    result.push(Token::DirectedEdge);
                }
                Some('-') => {
                    input.next();
                    result.push(Token::UndirectedEdge);
                }
                Some(next) if next.is_ascii_digit() || next == '.' => {
                    result.push(Token::Id(bare('-', &mut input)));
                }
                _ => return Err(()),
            },
            '"' => result.push(Token::Id(quoted(&mut input)?)),
            '<' | '#' => return Err(()),
            ch if id_start(ch) || ch.is_ascii_digit() || ch == '.' => {
                result.push(Token::Id(bare(ch, &mut input)));
            }
            _ => return Err(()),
        }
    }
    Ok(result)
}

fn id_start(ch: char) -> bool {
    ch == '_' || ch.is_alphabetic() || !ch.is_ascii()
}

fn id_continue(ch: char) -> bool {
    ch == '_' || ch == '.' || ch.is_alphanumeric() || !ch.is_ascii()
}

fn bare(first: char, input: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> String {
    let mut value = String::from(first);
    while let Some((_, next)) = input.peek() {
        if !id_continue(*next) {
            break;
        }
        value.push(*next);
        input.next();
    }
    value
}

fn quoted(input: &mut std::iter::Peekable<std::str::CharIndices<'_>>) -> Result<String, ()> {
    let mut value = String::new();
    while let Some((_, ch)) = input.next() {
        match ch {
            '"' => return Ok(value),
            '\\' => match input.next() {
                Some((_, '\n')) => {}
                Some((_, escaped)) => value.push(escaped),
                None => return Err(()),
            },
            _ => value.push(ch),
        }
    }
    Err(())
}

struct Parser {
    tokens: Vec<Token>,
    position: usize,
    directed: bool,
    attributes: Vec<String>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens,
            position: 0,
            directed: false,
            attributes: Vec::new(),
        }
    }

    fn document(mut self) -> Result<Document, ()> {
        if self.peek_id("strict") {
            self.position += 1;
        }
        self.directed = if self.peek_id("digraph") {
            self.position += 1;
            true
        } else if self.peek_id("graph") {
            self.position += 1;
            false
        } else {
            return Err(());
        };
        if matches!(self.peek(), Some(Token::Id(_))) {
            self.position += 1;
        }
        self.expect(Token::LeftBrace)?;
        self.statement_list()?;
        self.expect(Token::RightBrace)?;
        if self.peek().is_some() {
            return Err(());
        }
        Ok(Document {
            attributes: self.attributes,
        })
    }

    fn statement_list(&mut self) -> Result<(), ()> {
        loop {
            self.separators();
            if matches!(self.peek(), Some(Token::RightBrace)) {
                return Ok(());
            }
            if self.peek().is_none() {
                return Err(());
            }
            self.statement()?;
            self.separators();
        }
    }

    fn statement(&mut self) -> Result<(), ()> {
        if self.starts_subgraph() {
            self.subgraph()?;
            if self.edge_operator() {
                self.edge_rhs()?;
                self.attribute_lists()?;
            }
            return Ok(());
        }
        if ["graph", "node", "edge"]
            .iter()
            .any(|keyword| self.peek_id(keyword))
            && matches!(self.tokens.get(self.position + 1), Some(Token::LeftBracket))
        {
            self.position += 1;
            self.attribute_lists_required()?;
            return Ok(());
        }
        let name = self.node_id()?;
        if matches!(self.peek(), Some(Token::Equal)) {
            self.position += 1;
            self.id()?;
            self.attributes.push(name);
            return Ok(());
        }
        if self.edge_operator() {
            self.edge_rhs()?;
        }
        self.attribute_lists()
    }

    fn edge_rhs(&mut self) -> Result<(), ()> {
        let mut count = 0;
        while self.edge_operator() {
            let directed = matches!(self.peek(), Some(Token::DirectedEdge));
            if directed != self.directed {
                return Err(());
            }
            self.position += 1;
            if self.starts_subgraph() {
                self.subgraph()?;
            } else {
                self.node_id()?;
            }
            count += 1;
        }
        (count > 0).then_some(()).ok_or(())
    }

    fn subgraph(&mut self) -> Result<(), ()> {
        if self.peek_id("subgraph") {
            self.position += 1;
            if matches!(self.peek(), Some(Token::Id(_))) {
                self.position += 1;
            }
        }
        self.expect(Token::LeftBrace)?;
        self.statement_list()?;
        self.expect(Token::RightBrace)
    }

    fn starts_subgraph(&self) -> bool {
        matches!(self.peek(), Some(Token::LeftBrace))
            || (self.peek_id("subgraph")
                && (matches!(self.tokens.get(self.position + 1), Some(Token::LeftBrace))
                    || matches!(
                        (
                            self.tokens.get(self.position + 1),
                            self.tokens.get(self.position + 2)
                        ),
                        (Some(Token::Id(_)), Some(Token::LeftBrace))
                    )))
    }

    fn node_id(&mut self) -> Result<String, ()> {
        let name = self.id()?;
        if matches!(self.peek(), Some(Token::Colon)) {
            self.position += 1;
            self.id()?;
            if matches!(self.peek(), Some(Token::Colon)) {
                self.position += 1;
                self.id()?;
            }
        }
        Ok(name)
    }

    fn attribute_lists_required(&mut self) -> Result<(), ()> {
        if !matches!(self.peek(), Some(Token::LeftBracket)) {
            return Err(());
        }
        self.attribute_lists()
    }

    fn attribute_lists(&mut self) -> Result<(), ()> {
        while matches!(self.peek(), Some(Token::LeftBracket)) {
            self.position += 1;
            loop {
                self.separators();
                if matches!(self.peek(), Some(Token::RightBracket)) {
                    self.position += 1;
                    break;
                }
                let name = self.id()?;
                self.expect(Token::Equal)?;
                self.id()?;
                self.attributes.push(name);
                self.separators();
            }
        }
        Ok(())
    }

    fn id(&mut self) -> Result<String, ()> {
        match self.tokens.get(self.position) {
            Some(Token::Id(value)) => {
                self.position += 1;
                Ok(value.clone())
            }
            _ => Err(()),
        }
    }

    fn separators(&mut self) {
        while matches!(self.peek(), Some(Token::Semicolon | Token::Comma)) {
            self.position += 1;
        }
    }

    fn edge_operator(&self) -> bool {
        matches!(
            self.peek(),
            Some(Token::DirectedEdge | Token::UndirectedEdge)
        )
    }

    fn peek_id(&self, expected: &str) -> bool {
        matches!(self.peek(), Some(Token::Id(value)) if value.eq_ignore_ascii_case(expected))
    }

    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.position)
    }

    fn expect(&mut self, expected: Token) -> Result<(), ()> {
        if self.peek() == Some(&expected) {
            self.position += 1;
            Ok(())
        } else {
            Err(())
        }
    }
}
