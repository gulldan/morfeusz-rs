use crate::{CaseHandling, Charset, Error, Result, TokenNumbering, WhitespaceHandling};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NumberingScope {
    Separate,
    Continuous,
}

impl Default for NumberingScope {
    fn default() -> Self {
        Self::Separate
    }
}

impl From<TokenNumbering> for NumberingScope {
    fn from(value: TokenNumbering) -> Self {
        match value {
            TokenNumbering::Separate => Self::Separate,
            TokenNumbering::Continuous => Self::Continuous,
        }
    }
}

impl From<NumberingScope> for TokenNumbering {
    fn from(value: NumberingScope) -> Self {
        match value {
            NumberingScope::Separate => Self::Separate,
            NumberingScope::Continuous => Self::Continuous,
        }
    }
}

/// Segmentation options (`aggl`/`praet`). A `None` field means "use the
/// dictionary's own default option" — matching C++ where unset options are
/// initialized from the dictionary on `setDictionary`. Explicitly setting an
/// option overrides that default. The default preset leaves both unset, so a
/// freshly loaded dictionary behaves exactly like the C++ default.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SegmentationPreset {
    aggl: Option<String>,
    praet: Option<String>,
}

impl SegmentationPreset {
    pub fn new(aggl: impl Into<String>, praet: impl Into<String>) -> Result<Self> {
        let preset = Self {
            aggl: Some(aggl.into()),
            praet: Some(praet.into()),
        };
        preset.validate()?;
        Ok(preset)
    }

    /// The explicitly-set agglutination option, or `None` to use the
    /// dictionary's default.
    pub fn aggl(&self) -> Option<&str> {
        self.aggl.as_deref()
    }

    /// The explicitly-set praet option, or `None` to use the dictionary's
    /// default.
    pub fn praet(&self) -> Option<&str> {
        self.praet.as_deref()
    }

    pub fn with_aggl(mut self, aggl: impl Into<String>) -> Result<Self> {
        self.aggl = Some(aggl.into());
        self.validate()?;
        Ok(self)
    }

    pub fn with_praet(mut self, praet: impl Into<String>) -> Result<Self> {
        self.praet = Some(praet.into());
        self.validate()?;
        Ok(self)
    }

    fn validate(&self) -> Result<()> {
        if let Some(aggl) = &self.aggl {
            if !matches!(aggl.as_str(), "strict" | "permissive" | "isolated") {
                return Err(Error::InvalidArgument(format!(
                    "Invalid agglutination option: {aggl}"
                )));
            }
        }
        if let Some(praet) = &self.praet {
            if !matches!(praet.as_str(), "split" | "composite") {
                return Err(Error::InvalidArgument(format!(
                    "Invalid praet option: {praet}"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    charset: Charset,
    case_handling: CaseHandling,
    numbering: NumberingScope,
    whitespace: WhitespaceHandling,
    segmentation: SegmentationPreset,
    debug: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            charset: Charset::default(),
            case_handling: CaseHandling::default(),
            numbering: NumberingScope::default(),
            whitespace: WhitespaceHandling::default(),
            segmentation: SegmentationPreset::default(),
            debug: false,
        }
    }
}

impl Config {
    pub fn charset(&self) -> Charset {
        self.charset
    }

    pub fn case_handling(&self) -> CaseHandling {
        self.case_handling
    }

    pub fn numbering(&self) -> NumberingScope {
        self.numbering
    }

    pub fn whitespace(&self) -> WhitespaceHandling {
        self.whitespace
    }

    pub fn segmentation(&self) -> &SegmentationPreset {
        &self.segmentation
    }

    pub fn debug(&self) -> bool {
        self.debug
    }

    pub fn with_charset(mut self, charset: Charset) -> Self {
        self.charset = charset;
        self
    }

    pub fn with_case_handling(mut self, case_handling: CaseHandling) -> Self {
        self.case_handling = case_handling;
        self
    }

    pub fn with_numbering(mut self, numbering: NumberingScope) -> Self {
        self.numbering = numbering;
        self
    }

    pub fn with_whitespace(mut self, whitespace: WhitespaceHandling) -> Self {
        self.whitespace = whitespace;
        self
    }

    pub fn with_segmentation(mut self, segmentation: SegmentationPreset) -> Self {
        self.segmentation = segmentation;
        self
    }

    pub fn with_debug(mut self, debug: bool) -> Self {
        self.debug = debug;
        self
    }
}
