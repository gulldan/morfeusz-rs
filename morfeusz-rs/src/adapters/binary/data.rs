use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsaImplementation {
    Simple,
    SimpleWithTransitionData,
    VLength1,
    VLength2,
}

impl FsaImplementation {
    fn from_code(code: u8) -> Result<Self> {
        match code {
            0 => Ok(Self::Simple),
            128 => Ok(Self::SimpleWithTransitionData),
            1 => Ok(Self::VLength1),
            2 => Ok(Self::VLength2),
            _ => Err(Error::invalid_dictionary(format!(
                "unsupported FSA implementation code: {code}"
            ))),
        }
    }
}

#[derive(Debug, Clone)]
pub struct BinaryDictionaryData {
    // Keep the original allocation behind `Arc<Vec<_>>`: converting a `Vec`
    // into `Arc<[u8]>` has to allocate and copy the multi-megabyte payload.
    // The vector itself is immutable after loading and is shared by all forks.
    bytes: Arc<Vec<u8>>,
    implementation: FsaImplementation,
    fsa_range: Range<usize>,
    epilogue_offset: usize,
    id_resolver_offset: usize,
    segmentation_rules_offset: usize,
    dict_id: String,
    copyright: String,
}

impl BinaryDictionaryData {
    pub fn from_path(path: impl AsRef<Path>) -> Result<Self> {
        Self::from_bytes(fs::read(path)?)
    }

    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        validate_min_len(&bytes, FSA_DATA_OFFSET, "dictionary prologue")?;
        let magic = read_u32_at(&bytes, 0, "magic number")?;
        if magic != MAGIC_NUMBER {
            return Err(Error::invalid_dictionary(format!(
                "invalid dictionary magic: 0x{magic:08x}"
            )));
        }

        let version = bytes[VERSION_NUM_OFFSET];
        if version != VERSION_NUM {
            return Err(Error::invalid_dictionary(format!(
                "unsupported dictionary version: {version}"
            )));
        }

        let implementation = FsaImplementation::from_code(bytes[IMPLEMENTATION_NUM_OFFSET])?;
        let fsa_size = read_u32_at(&bytes, FSA_DATA_SIZE_OFFSET, "FSA size")? as usize;
        let fsa_end = FSA_DATA_OFFSET
            .checked_add(fsa_size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| Error::invalid_dictionary("FSA data exceeds dictionary size"))?;
        let fsa_range = FSA_DATA_OFFSET..fsa_end;
        let epilogue_offset = fsa_end;
        validate_min_len(&bytes, epilogue_offset + 4, "dictionary epilogue offset")?;

        let epilogue = &bytes[epilogue_offset..];
        let segmentation_rules_offset =
            read_u32_at(epilogue, 0, "segmentation rules offset")? as usize;
        let segmentation_rules_start = epilogue_offset
            .checked_add(4)
            .and_then(|offset| offset.checked_add(segmentation_rules_offset))
            .filter(|offset| *offset <= bytes.len())
            .ok_or_else(|| {
                Error::invalid_dictionary("segmentation rules offset exceeds dictionary size")
            })?;

        let metadata_start = epilogue_offset + 4;
        let (dict_id, copyright_start) = read_c_string_at(&bytes, metadata_start, "dictionary id")?;
        let (copyright, id_resolver_offset) =
            read_c_string_at(&bytes, copyright_start, "dictionary copyright")?;

        Ok(Self {
            bytes: Arc::new(bytes),
            implementation,
            fsa_range,
            epilogue_offset,
            id_resolver_offset,
            segmentation_rules_offset: segmentation_rules_start,
            dict_id,
            copyright,
        })
    }

    pub fn version(&self) -> u8 {
        VERSION_NUM
    }

    pub fn implementation(&self) -> FsaImplementation {
        self.implementation
    }

    pub fn dict_id(&self) -> &str {
        &self.dict_id
    }

    pub fn copyright(&self) -> &str {
        &self.copyright
    }

    pub fn fsa_data(&self) -> &[u8] {
        &self.bytes[self.fsa_range.clone()]
    }

    pub fn epilogue(&self) -> &[u8] {
        &self.bytes[self.epilogue_offset..]
    }

    pub fn segmentation_rules_data(&self) -> &[u8] {
        &self.bytes[self.segmentation_rules_offset..]
    }

    pub fn segmentation_metadata(&self) -> Result<SegmentationMetadata> {
        parse_segmentation_metadata(self.segmentation_rules_data())
    }

    pub fn id_resolver(&self) -> Result<IdResolver> {
        let mut cursor = self.id_resolver_offset;
        let limit = self.segmentation_rules_offset;
        let mut resolver = IdResolver::default();

        let (tagset_id, next) = read_c_string_at_limit(&self.bytes, cursor, limit, "tagset id")?;
        resolver.set_tagset_id(tagset_id);
        cursor = next;

        read_id_string_table(&self.bytes, &mut cursor, limit, |id, value| {
            resolver.set_tag(id, value);
        })?;
        read_id_string_table(&self.bytes, &mut cursor, limit, |id, value| {
            resolver.set_name(id, value);
        })?;
        read_id_string_table(&self.bytes, &mut cursor, limit, |id, value| {
            resolver.set_labels_in_order(id, value);
        })?;

        Ok(resolver)
    }

    pub fn fsa(&self) -> Result<BinaryFsa<'_>> {
        match self.implementation {
            FsaImplementation::Simple => {
                Ok(BinaryFsa::Simple(SimpleFsa::new(self.fsa_data(), false)?))
            }
            FsaImplementation::SimpleWithTransitionData => {
                Ok(BinaryFsa::Simple(SimpleFsa::new(self.fsa_data(), true)?))
            }
            FsaImplementation::VLength1 => {
                Ok(BinaryFsa::VLength1(VLength1Fsa::new(self.fsa_data())?))
            }
            FsaImplementation::VLength2 => {
                Ok(BinaryFsa::VLength2(VLength2Fsa::new(self.fsa_data())))
            }
        }
    }

    pub(super) fn fsa_unchecked(&self) -> BinaryFsa<'_> {
        match self.implementation {
            FsaImplementation::Simple => BinaryFsa::Simple(SimpleFsa {
                data: self.fsa_data(),
                transition_size: SIMPLE_TRANSITION_SIZE,
            }),
            FsaImplementation::SimpleWithTransitionData => BinaryFsa::Simple(SimpleFsa {
                data: self.fsa_data(),
                transition_size: SIMPLE_TRANSDUCER_TRANSITION_SIZE,
            }),
            FsaImplementation::VLength1 => {
                BinaryFsa::VLength1(VLength1Fsa::from_data_unchecked(self.fsa_data()))
            }
            FsaImplementation::VLength2 => BinaryFsa::VLength2(VLength2Fsa::new(self.fsa_data())),
        }
    }

    pub fn vlength2_fsa(&self) -> Result<VLength2Fsa<'_>> {
        match self.fsa()? {
            BinaryFsa::VLength2(fsa) => Ok(fsa),
            _ => Err(Error::invalid_dictionary(
                "dictionary does not use VLength2 FSA encoding",
            )),
        }
    }
}
