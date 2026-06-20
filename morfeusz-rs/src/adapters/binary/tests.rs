use super::*;
use crate::Engine;

#[test]
fn parses_binary_dictionary_sections() {
    let bytes = minimal_dictionary_bytes();

    let dictionary = BinaryDictionaryData::from_bytes(bytes).unwrap();

    assert_eq!(dictionary.version(), VERSION_NUM);
    assert_eq!(dictionary.implementation(), FsaImplementation::VLength2);
    assert_eq!(dictionary.fsa_data(), [0xaa, 0xbb]);
    assert_eq!(dictionary.dict_id(), "test-dict");
    assert_eq!(dictionary.copyright(), "copyright");
    assert_eq!(
        dictionary.segmentation_rules_data(),
        segmentation_metadata_bytes()
    );
}

#[test]
fn rejects_invalid_magic() {
    let mut bytes = minimal_dictionary_bytes();
    bytes[0] = 0;

    assert!(matches!(
        BinaryDictionaryData::from_bytes(bytes),
        Err(Error::InvalidDictionary(message)) if message.contains("magic")
    ));
}

#[test]
fn rejects_truncated_fsa_data() {
    let mut bytes = minimal_dictionary_bytes();
    bytes[FSA_DATA_SIZE_OFFSET..FSA_DATA_SIZE_OFFSET + 4]
        .copy_from_slice(&999_999_u32.to_be_bytes());

    assert!(matches!(
        BinaryDictionaryData::from_bytes(bytes),
        Err(Error::InvalidDictionary(message)) if message.contains("FSA data")
    ));
}

#[test]
fn rejects_unknown_implementation_code() {
    let mut bytes = minimal_dictionary_bytes();
    bytes[IMPLEMENTATION_NUM_OFFSET] = 9;

    assert!(matches!(
        BinaryDictionaryData::from_bytes(bytes),
        Err(Error::InvalidDictionary(message)) if message.contains("implementation")
    ));
}

#[test]
fn recognizes_raw_vlength2_payload() {
    let fsa = VLength2Fsa::new(&[
        b'a',
        V2_LAST_FLAG | V2_ACCEPTING_FLAG,
        0,
        3,
        0xde,
        0xad,
        0xbe,
        V2_LAST_FLAG,
    ]);

    let matched = fsa.try_recognize(b"a").unwrap().unwrap();

    assert_eq!(matched.state_offset, 2);
    assert_eq!(matched.value, [0xde, 0xad, 0xbe]);
    assert!(fsa.try_recognize(b"b").unwrap().is_none());
}

#[test]
fn recognizes_raw_simple_payload() {
    let fsa = SimpleFsa::new(&[0x01, b'a', 0, 0, 5, 0x80, 0, 1, 0x11], false).unwrap();

    let matched = fsa.try_recognize(b"a").unwrap().unwrap();

    assert_eq!(matched.state_offset, 5);
    assert_eq!(matched.value, [0x11]);
    assert!(fsa.try_recognize(b"b").unwrap().is_none());
}

#[test]
fn skips_accepting_payload_before_following_simple_transitions() {
    let fsa = SimpleFsa::new(
        &[
            0x01, b'a', 0, 0, 5, 0x81, 0, 1, 0x11, b'b', 0, 0, 13, 0x80, 0, 1, 0x22,
        ],
        false,
    )
    .unwrap();

    let prefix = fsa.try_recognize(b"a").unwrap().unwrap();
    let full = fsa.try_recognize(b"ab").unwrap().unwrap();

    assert_eq!(prefix.value, [0x11]);
    assert_eq!(full.value, [0x22]);
    assert!(fsa.try_recognize(b"ac").unwrap().is_none());
}

#[test]
fn recognizes_simple_payload_with_transition_data() {
    let fsa = SimpleFsa::new(&[0x01, b'a', 0, 0, 6, 0xee, 0x80, 0, 1, 0x11], true).unwrap();

    let matched = fsa.try_recognize(b"a").unwrap().unwrap();

    assert_eq!(matched.state_offset, 6);
    assert_eq!(matched.value, [0x11]);
}

#[test]
fn recognizes_raw_vlength1_payload() {
    let fsa_data = vlength1_fsa_data(&[0x01, 0x00, b'a', 0x80, 0, 1, 0x11]);
    let fsa = VLength1Fsa::new(&fsa_data).unwrap();

    let matched = fsa.try_recognize(b"a").unwrap().unwrap();

    assert_eq!(matched.state_offset, 3);
    assert_eq!(matched.value, [0x11]);
    assert!(fsa.try_recognize(b"b").unwrap().is_none());
}

#[test]
fn rejects_truncated_vlength1_transition_at_load() {
    let fsa_data = vlength1_fsa_data(&[0x01, 0x00]);

    assert!(matches!(
        VLength1Fsa::new(&fsa_data),
        Err(Error::InvalidDictionary(message)) if message.contains("transition label")
    ));
}

#[test]
fn rejects_vlength1_target_past_data_at_load() {
    let fsa_data = vlength1_fsa_data(&[0x01, 0x05, 0xff]);

    assert!(matches!(
        VLength1Fsa::new(&fsa_data),
        Err(Error::InvalidDictionary(message)) if message.contains("target state")
    ));
}

#[test]
fn normalized_identity_span_preserves_original_case_offsets() {
    let original = "ABC";
    let normalized = lowercase_with_original_boundaries(original);

    assert_eq!(normalized.as_str(), "abc");
    assert_eq!(normalized.original_span(original, 0, 1), Some(("A", 0, 1)));
    assert_eq!(normalized.original_span(original, 1, 3), Some(("BC", 1, 3)));
}

#[test]
fn normalized_contracting_lowercase_span_uses_original_boundaries() {
    // 'İ' (U+0130, 2 bytes) lowercases to a single 'i' (1 byte) via the
    // Morfeusz table — NOT Unicode's two-codepoint "i\u{0307}". The boundary
    // table must still map back to the original 2-byte span.
    let original = "\u{0130}x";
    let normalized = lowercase_with_original_boundaries(original);

    assert_eq!(normalized.as_str(), "ix");
    assert_eq!(
        normalized.original_span(original, 0, 1),
        Some(("\u{0130}", 0, 2))
    );
    assert_eq!(normalized.original_span(original, 1, 2), Some(("x", 2, 3)));
}

#[test]
fn skips_accepting_payload_before_following_vlength1_transitions() {
    let fsa_data = vlength1_fsa_data(&[
        0x01, 0x00, b'a', 0x81, 0, 1, 0x11, 0x00, b'b', 0x80, 0, 1, 0x22,
    ]);
    let fsa = VLength1Fsa::new(&fsa_data).unwrap();

    let prefix = fsa.try_recognize(b"a").unwrap().unwrap();
    let full = fsa.try_recognize(b"ab").unwrap().unwrap();

    assert_eq!(prefix.value, [0x11]);
    assert_eq!(full.value, [0x22]);
    assert!(fsa.try_recognize(b"ac").unwrap().is_none());
}

#[test]
fn collects_vlength1_prefix_matches() {
    let fsa_data = vlength1_fsa_data(&[
        0x01, 0x00, b'a', 0x81, 0, 1, 0x11, 0x00, b'b', 0x80, 0, 1, 0x22,
    ]);
    let fsa = VLength1Fsa::new(&fsa_data).unwrap();

    let matches = fsa.prefix_matches(b"ab").unwrap();

    assert_eq!(
        matches,
        [
            RawFsaPrefixMatch {
                input_end: 1,
                state_offset: 3,
                value: &[0x11],
            },
            RawFsaPrefixMatch {
                input_end: 2,
                state_offset: 9,
                value: &[0x22],
            }
        ]
    );
}

#[test]
fn skips_accepting_payload_before_following_vlength2_transitions() {
    let fsa = VLength2Fsa::new(&[
        b'a',
        V2_LAST_FLAG | V2_ACCEPTING_FLAG,
        0,
        1,
        0x11,
        b'b',
        V2_LAST_FLAG | V2_ACCEPTING_FLAG,
        0,
        1,
        0x22,
        V2_LAST_FLAG,
    ]);

    let prefix = fsa.try_recognize(b"a").unwrap().unwrap();
    let full = fsa.try_recognize(b"ab").unwrap().unwrap();

    assert_eq!(prefix.value, [0x11]);
    assert_eq!(full.value, [0x22]);
    assert!(fsa.try_recognize(b"ac").unwrap().is_none());
}

#[test]
fn collects_vlength2_prefix_matches() {
    let fsa = VLength2Fsa::new(&[
        b'a',
        V2_LAST_FLAG | V2_ACCEPTING_FLAG,
        0,
        1,
        0x11,
        b'b',
        V2_LAST_FLAG | V2_ACCEPTING_FLAG,
        0,
        1,
        0x22,
        0,
        V2_LAST_FLAG,
    ]);

    let matches = fsa.prefix_matches(b"ab").unwrap();

    assert_eq!(
        matches,
        [
            RawFsaPrefixMatch {
                input_end: 1,
                state_offset: 2,
                value: &[0x11],
            },
            RawFsaPrefixMatch {
                input_end: 2,
                state_offset: 7,
                value: &[0x22],
            }
        ]
    );
}

#[test]
fn reads_raw_interpretation_groups() {
    let payload = [7, 0, 2, 0xaa, 0xbb, 9, 0, 1, 0xcc];

    let groups = read_raw_interps_groups(&payload).unwrap();

    assert_eq!(
        groups,
        [
            RawInterpsGroup {
                segment_type: 7,
                data: &[0xaa, 0xbb]
            },
            RawInterpsGroup {
                segment_type: 9,
                data: &[0xcc]
            }
        ]
    );
}

#[test]
fn rejects_truncated_interpretation_group() {
    let payload = [7, 0, 3, 0xaa];

    assert!(matches!(
        read_raw_interps_groups(&payload),
        Err(Error::InvalidDictionary(message)) if message.contains("group data")
    ));
}

#[test]
fn decodes_compressed_analyzer_interpretation_record() {
    let group = RawInterpsGroup {
        segment_type: 4,
        data: &[ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER, 0, 0, 0, 42, 7, 0, 9],
    };

    let decoded = decode_analyzer_interpretations(group).unwrap();

    assert_eq!(
        decoded,
        [EncodedAnalyzerInterpretation {
            orth_case_pattern: Vec::new(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 0,
                suffix_to_add: String::new(),
                case_pattern: Vec::new(),
                prefix_to_add: String::new(),
            },
            tag_id: 42,
            name_id: 7,
            labels_id: 9,
        }]
    );
}

#[test]
fn decodes_analyzer_interps_groups_with_segment_types() {
    let payload = analyzer_groups_payload(&[4, 8]);

    let groups = decode_analyzer_interps_groups(&payload).unwrap();

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].segment_type, 4);
    assert_eq!(groups[0].interpretations[0].tag_id, 42);
    assert_eq!(groups[1].segment_type, 8);
    assert_eq!(groups[1].interpretations[0].name_id, 7);
}

#[test]
fn analyzer_decode_cache_reuses_groups_across_words() {
    let shared = SharedAnalyzerGroupDecodeCache::default();
    let group = RawInterpsGroup {
        segment_type: 4,
        data: &[ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER, 0, 0, 0, 42, 7, 0, 9],
    };
    let mut first_word = AnalyzerGroupDecodeCache::with_capacity(shared.clone(), 1);

    let first_index = first_word.get_or_decode((123, 0), group).unwrap();

    assert_eq!(first_word.interpretations(first_index)[0].tag_id, 42);

    let invalid_if_decoded = RawInterpsGroup {
        segment_type: 4,
        data: &[],
    };
    let mut second_word = AnalyzerGroupDecodeCache::with_capacity(shared, 1);
    let second_index = second_word
        .get_or_decode((123, 0), invalid_if_decoded)
        .unwrap();

    assert_eq!(second_word.interpretations(second_index)[0].name_id, 7);
}

#[test]
fn decodes_explicit_analyzer_case_patterns_and_prefix_cut() {
    let group = RawInterpsGroup {
        segment_type: 4,
        data: &[
            PREFIX_CUT_MASK,
            0,
            CASE_PATTERN_MIXED,
            1,
            1,
            3,
            1,
            b'x',
            0,
            CASE_PATTERN_UPPER_PREFIX,
            2,
            0,
            42,
            7,
            0,
            9,
        ],
    };

    let decoded = decode_analyzer_interpretations(group).unwrap();

    assert_eq!(decoded.len(), 1);
    assert_eq!(decoded[0].orth_case_pattern, [false, true]);
    assert_eq!(decoded[0].form.prefix_to_cut, 3);
    assert_eq!(decoded[0].form.suffix_to_cut, 1);
    assert_eq!(decoded[0].form.suffix_to_add, "x");
    assert_eq!(decoded[0].form.case_pattern, [true, true]);
    assert_eq!(decoded[0].tag_id, 42);
    assert_eq!(decoded[0].name_id, 7);
    assert_eq!(decoded[0].labels_id, 9);
}

#[test]
fn decodes_generator_interpretation_record() {
    let group = RawInterpsGroup {
        segment_type: 4,
        data: &[
            b's', b'1', 0, b'p', b'r', b'e', 0, 2, b's', b'u', b'f', 0, 0, 42, 7, 0, 9,
        ],
    };

    let decoded = decode_generator_interpretations(group).unwrap();

    assert_eq!(
        decoded,
        [EncodedGeneratorInterpretation {
            homonym_id: "s1".to_owned(),
            form: EncodedForm {
                prefix_to_cut: 0,
                suffix_to_cut: 2,
                suffix_to_add: "suf".to_owned(),
                case_pattern: Vec::new(),
                prefix_to_add: "pre".to_owned(),
            },
            tag_id: 42,
            name_id: 7,
            labels_id: 9,
        }]
    );
}

#[test]
fn decodes_generator_interps_groups_with_segment_types() {
    let payload = generator_groups_payload(&[4, 8]);

    let groups = decode_generator_interps_groups(&payload).unwrap();

    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].segment_type, 4);
    assert_eq!(groups[0].interpretations[0].form.suffix_to_cut, 0);
    assert_eq!(groups[1].segment_type, 8);
    assert_eq!(groups[1].interpretations[0].tag_id, 42);
}

#[test]
fn generator_decode_cache_reuses_groups_across_lemmas() {
    let shared = SharedGeneratorGroupDecodeCache::default();
    let group = RawInterpsGroup {
        segment_type: 4,
        data: &[
            b's', b'1', 0, b'p', b'r', b'e', 0, 2, b's', b'u', b'f', 0, 0, 42, 7, 0, 9,
        ],
    };
    let mut first_lemma = GeneratorGroupDecodeCache::with_capacity(shared.clone(), 1);

    let first_index = first_lemma.get_or_decode((456, 0), group).unwrap();

    assert_eq!(
        first_lemma.interpretations(first_index)[0]
            .form
            .prefix_to_add,
        "pre"
    );

    let invalid_if_decoded = RawInterpsGroup {
        segment_type: 4,
        data: &[],
    };
    let mut second_lemma = GeneratorGroupDecodeCache::with_capacity(shared, 1);
    let second_index = second_lemma
        .get_or_decode((456, 0), invalid_if_decoded)
        .unwrap();

    assert_eq!(second_lemma.interpretations(second_index)[0].tag_id, 42);
}

#[test]
fn applies_generator_form_with_unicode_codepoint_suffix_cut() {
    let interp = EncodedGeneratorInterpretation {
        homonym_id: "s1".to_owned(),
        form: EncodedForm {
            prefix_to_cut: 0,
            suffix_to_cut: 1,
            suffix_to_add: "ego".to_owned(),
            case_pattern: Vec::new(),
            prefix_to_add: "naj".to_owned(),
        },
        tag_id: 42,
        name_id: 7,
        labels_id: 9,
    };

    let decoded = interp.to_morph_interpretation("żółty", 2, 3).unwrap();

    assert_eq!(decoded.start_node, 2);
    assert_eq!(decoded.end_node, 3);
    assert_eq!(decoded.orth, "najżółtego");
    assert_eq!(decoded.lemma, "żółty:s1");
    assert_eq!(decoded.tag_id, 42);
    assert_eq!(decoded.name_id, 7);
    assert_eq!(decoded.labels_id, 9);
}

#[test]
fn applies_generator_form_without_suffix_cut_to_unicode_lemma() {
    let interp = EncodedGeneratorInterpretation {
        homonym_id: String::new(),
        form: EncodedForm {
            prefix_to_cut: 0,
            suffix_to_cut: 0,
            suffix_to_add: String::new(),
            case_pattern: Vec::new(),
            prefix_to_add: String::new(),
        },
        tag_id: 42,
        name_id: 7,
        labels_id: 9,
    };

    let decoded = interp.to_morph_interpretation("żółty", 2, 3).unwrap();

    assert_eq!(decoded.orth, "żółty");
    assert_eq!(decoded.lemma, "żółty");
}

#[test]
fn applies_generator_form_with_ascii_suffix_cut() {
    let interp = EncodedGeneratorInterpretation {
        homonym_id: String::new(),
        form: EncodedForm {
            prefix_to_cut: 0,
            suffix_to_cut: 2,
            suffix_to_add: "ed".to_owned(),
            case_pattern: Vec::new(),
            prefix_to_add: String::new(),
        },
        tag_id: 42,
        name_id: 7,
        labels_id: 9,
    };

    let decoded = interp.to_morph_interpretation("testxx", 2, 3).unwrap();

    assert_eq!(decoded.orth, "tested");
    assert_eq!(decoded.lemma, "testxx");
}

#[test]
fn rejects_generator_suffix_cut_longer_than_lemma() {
    let interp = EncodedGeneratorInterpretation {
        homonym_id: String::new(),
        form: EncodedForm {
            prefix_to_cut: 0,
            suffix_to_cut: 2,
            suffix_to_add: String::new(),
            case_pattern: Vec::new(),
            prefix_to_add: String::new(),
        },
        tag_id: 0,
        name_id: 0,
        labels_id: 0,
    };

    assert!(matches!(
        interp.to_morph_interpretation("a", 0, 0),
        Err(Error::InvalidDictionary(message)) if message.contains("cannot cut")
    ));
}

#[test]
fn applies_analyzer_form_with_unicode_case_pattern() {
    let interp = EncodedAnalyzerInterpretation {
        orth_case_pattern: Vec::new(),
        form: EncodedForm {
            prefix_to_cut: 0,
            suffix_to_cut: 1,
            suffix_to_add: "ego".to_owned(),
            case_pattern: vec![true],
            prefix_to_add: String::new(),
        },
        tag_id: 42,
        name_id: 7,
        labels_id: 9,
    };

    let decoded = interp.to_morph_interpretation("ŻÓŁTY", 2, 3).unwrap();

    assert_eq!(decoded.start_node, 2);
    assert_eq!(decoded.end_node, 3);
    assert_eq!(decoded.orth, "ŻÓŁTY");
    assert_eq!(decoded.lemma, "Żółtego");
    assert_eq!(decoded.tag_id, 42);
    assert_eq!(decoded.name_id, 7);
    assert_eq!(decoded.labels_id, 9);
}

#[test]
fn applies_analyzer_identity_form_to_unicode_lowercase_orth() {
    let interp = EncodedAnalyzerInterpretation {
        orth_case_pattern: Vec::new(),
        form: EncodedForm {
            prefix_to_cut: 0,
            suffix_to_cut: 0,
            suffix_to_add: String::new(),
            case_pattern: Vec::new(),
            prefix_to_add: String::new(),
        },
        tag_id: 42,
        name_id: 7,
        labels_id: 9,
    };

    let decoded = interp.to_morph_interpretation("żółty", 2, 3).unwrap();

    assert_eq!(decoded.orth, "żółty");
    assert_eq!(decoded.lemma, "żółty");
}

#[test]
fn analyzer_orth_context_reuses_unicode_lengths_for_binary_forms() {
    let context = AnalyzerOrthContext::new("ŻÓŁTY");
    let form = BinaryAnalyzerForm {
        prefix_to_cut: 0,
        suffix_to_cut: 1,
        suffix_to_add: "ego".to_owned(),
        case_pattern: BinaryCasePattern::UpperPrefix(1),
    };

    assert_eq!(context.original_codepoints_len, 5);
    assert_eq!(context.lowercase_codepoints_len, 5);
    assert!(!context.lowercases_to_self);
    assert_eq!(
        decode_analyzer_lemma_for_form_in_context(&context, &form).unwrap(),
        "Żółtego"
    );
}

#[test]
fn matches_analyzer_orth_case_pattern() {
    let interp = EncodedAnalyzerInterpretation {
        orth_case_pattern: vec![true, false, true],
        form: EncodedForm {
            prefix_to_cut: 0,
            suffix_to_cut: 0,
            suffix_to_add: String::new(),
            case_pattern: Vec::new(),
            prefix_to_add: String::new(),
        },
        tag_id: 0,
        name_id: 0,
        labels_id: 0,
    };

    assert!(interp.matches_orth_case("AbC"));
    assert!(interp.matches_orth_case("ABC"));
    assert!(interp.matches_orth_case("İxC"));
    assert!(!interp.matches_orth_case("abc"));
    assert!(!interp.matches_orth_case("Ab"));
    assert!(!interp.matches_orth_case("ßxC"));
}

#[test]
fn applies_analyzer_prefix_and_suffix_cuts() {
    let interp = EncodedAnalyzerInterpretation {
        orth_case_pattern: Vec::new(),
        form: EncodedForm {
            prefix_to_cut: 1,
            suffix_to_cut: 1,
            suffix_to_add: "ny".to_owned(),
            case_pattern: Vec::new(),
            prefix_to_add: String::new(),
        },
        tag_id: 1,
        name_id: 2,
        labels_id: 3,
    };

    let decoded = interp.to_morph_interpretation("ABCDE", 0, 1).unwrap();

    assert_eq!(decoded.lemma, "bcdny");
}

#[test]
fn rejects_analyzer_cuts_outside_orth() {
    let interp = EncodedAnalyzerInterpretation {
        orth_case_pattern: Vec::new(),
        form: EncodedForm {
            prefix_to_cut: 2,
            suffix_to_cut: 2,
            suffix_to_add: String::new(),
            case_pattern: Vec::new(),
            prefix_to_add: String::new(),
        },
        tag_id: 0,
        name_id: 0,
        labels_id: 0,
    };

    assert!(matches!(
        interp.to_morph_interpretation("a", 0, 1),
        Err(Error::InvalidDictionary(message)) if message.contains("cannot cut")
            || message.contains("prefix cut")
    ));
}

#[test]
fn reads_binary_id_resolver_tables() {
    let dictionary = BinaryDictionaryData::from_bytes(dictionary_bytes_with_id_resolver()).unwrap();

    let resolver = dictionary.id_resolver().unwrap();

    assert_eq!(resolver.tagset_id(), "test-tagset");
    assert_eq!(resolver.tag(42), Some("subst:sg:nom:m1"));
    assert_eq!(resolver.tag_id("subst:sg:nom:m1").unwrap(), 42);
    assert_eq!(resolver.name(7), Some("wlasna"));
    assert_eq!(resolver.name_id("wlasna").unwrap(), 7);
    assert_eq!(resolver.labels_as_string(9), Some("a|b"));
    assert_eq!(resolver.labels_id("a|b").unwrap(), 9);
    assert!(resolver.labels_id("b|a").is_err());
    assert!(resolver.labels(9).unwrap().contains("a"));
}

#[test]
fn reads_binary_segmentation_metadata() {
    let dictionary = BinaryDictionaryData::from_bytes(dictionary_bytes_with_id_resolver()).unwrap();

    let metadata = dictionary.segmentation_metadata().unwrap();

    assert_eq!(metadata.separators, [44, 46]);
    assert_eq!(metadata.fsa_variants.len(), 1);
    assert_eq!(
        metadata
            .available_options("aggl")
            .into_iter()
            .collect::<Vec<_>>(),
        ["permissive"]
    );
    assert_eq!(
        metadata
            .available_options("praet")
            .into_iter()
            .collect::<Vec<_>>(),
        ["split"]
    );
    assert_eq!(
        metadata.fsa_variants[0]
            .options
            .get("aggl")
            .map(String::as_str),
        Some("permissive")
    );
    assert_eq!(
        metadata.fsa_variants[0]
            .options
            .get("praet")
            .map(String::as_str),
        Some("split")
    );
    assert_eq!(metadata.fsa_variants[0].fsa, [1, 0]);
    assert_eq!(
        metadata.default_options.get("aggl").map(String::as_str),
        Some("permissive")
    );
    assert_eq!(
        metadata.default_options.get("praet").map(String::as_str),
        Some("split")
    );
    let default_variant = metadata.default_fsa_variant().unwrap();
    let rules_fsa = default_variant.rules_fsa().unwrap();
    assert_eq!(rules_fsa.initial_state(), SegmentationState::initial());
}

#[test]
fn selects_segmentation_fsa_variant_from_runtime_options() {
    let metadata = SegmentationMetadata {
        separators: Vec::new(),
        fsa_variants: vec![
            SegmentationFsaVariant {
                options: options_map(&[("aggl", "strict"), ("praet", "split")]),
                fsa: vec![0, 0],
            },
            SegmentationFsaVariant {
                options: options_map(&[("aggl", "permissive"), ("praet", "split")]),
                fsa: vec![0, 1, 4, 0, 0, 6, 1, 0],
            },
        ],
        default_options: options_map(&[("aggl", "strict"), ("praet", "split")]),
    };
    let dictionary_default = SegmentationPreset::default();
    let strict = SegmentationPreset::new("strict", "split").unwrap();
    let permissive = SegmentationPreset::new("permissive", "split").unwrap();
    let invalid_combo = SegmentationPreset::new("permissive", "composite").unwrap();

    assert_eq!(
        metadata
            .available_options("aggl")
            .into_iter()
            .collect::<Vec<_>>(),
        ["permissive", "strict"]
    );
    assert_eq!(
        metadata
            .available_options("praet")
            .into_iter()
            .collect::<Vec<_>>(),
        ["split"]
    );
    assert_eq!(
        segmentation_fsa_for_options(&metadata, &dictionary_default).unwrap(),
        Some([0, 0].as_slice())
    );
    assert_eq!(default_segmentation_fsa_variant_index(&metadata), Some(0));
    assert_eq!(
        segmentation_fsa_for_options(&metadata, &strict).unwrap(),
        Some([0, 0].as_slice())
    );
    assert_eq!(
        segmentation_fsa_for_options(&metadata, &permissive).unwrap(),
        Some([0, 1, 4, 0, 0, 6, 1, 0].as_slice())
    );
    assert!(matches!(
        segmentation_fsa_for_options(&metadata, &invalid_combo),
        Err(Error::InvalidArgument(message))
            if message.contains("aggl=permissive") && message.contains("praet=composite")
    ));
    assert!(matches!(
        validate_segmentation_options(&metadata, &invalid_combo, "praet", "composite"),
        Err(Error::InvalidArgument(message))
            if message.contains("Invalid \"praet\" option")
                && message.contains("\"split\"")
    ));

    for preset in [&dictionary_default, &strict, &permissive] {
        assert_eq!(
            metadata
                .fsa_variant_for_preset(preset)
                .map(|variant| variant.fsa.as_slice()),
            metadata
                .fsa_variant_for_options(&effective_segmentation_options(&metadata, preset))
                .map(|variant| variant.fsa.as_slice())
        );
    }
}

#[test]
fn rejects_trailing_segmentation_metadata_bytes() {
    let mut bytes = dictionary_bytes_with_id_resolver();
    bytes.push(0xff);
    let dictionary = BinaryDictionaryData::from_bytes(bytes).unwrap();

    assert!(matches!(
        dictionary.segmentation_metadata(),
        Err(Error::InvalidDictionary(message)) if message.contains("trailing")
    ));
}

#[test]
fn traverses_segmentation_rules_fsa() {
    let bytes = segmentation_rules_fsa_bytes();
    let fsa = SegmentationRulesFsa::new(&bytes).unwrap();
    let initial = fsa.initial_state();

    let terminal = fsa.proceed_to_next(4, initial, true).unwrap().unwrap();
    assert_eq!(
        terminal,
        SegmentationState {
            offset: 10,
            accepting: true,
            weak: false,
            shift_orth_from_previous: false,
            sink: true,
            failed: false,
        }
    );
    assert!(fsa.proceed_to_next(4, initial, false).unwrap().is_none());

    let mid = fsa.proceed_to_next(5, initial, false).unwrap().unwrap();
    assert_eq!(
        mid,
        SegmentationState {
            offset: 12,
            accepting: false,
            weak: false,
            shift_orth_from_previous: true,
            sink: false,
            failed: false,
        }
    );
    assert!(fsa.proceed_to_next(5, initial, true).unwrap().is_none());

    let final_state = fsa.proceed_to_next(6, mid, true).unwrap().unwrap();
    assert_eq!(
        final_state,
        SegmentationState {
            offset: 18,
            accepting: true,
            weak: true,
            shift_orth_from_previous: false,
            sink: true,
            failed: false,
        }
    );
    assert!(fsa.proceed_to_next(6, mid, false).unwrap().is_none());
    assert!(fsa.proceed_to_next(99, initial, false).unwrap().is_none());
}

#[test]
fn rejects_failed_segmentation_state_transition() {
    let bytes = segmentation_rules_fsa_bytes();
    let fsa = SegmentationRulesFsa::new(&bytes).unwrap();

    assert!(matches!(
        fsa.proceed_to_next(4, SegmentationState::failed(), true),
        Err(Error::InvalidArgument(message)) if message.contains("failed")
    ));
}

#[test]
fn rejects_segmentation_fsa_transition_past_data() {
    let bytes = [0, 1, 4, 0, 0, 10];

    assert!(matches!(
        SegmentationRulesFsa::new(&bytes),
        Err(Error::InvalidDictionary(message)) if message.contains("target state")
    ));
}

#[test]
fn binary_generator_lexicon_integrates_with_engine_generate() {
    let lexicon = BinaryGeneratorLexicon::from_bytes(binary_generator_dictionary_bytes()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let generated = engine.generate("kot").unwrap();
    let unknown = engine.generate("pies").unwrap();

    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].orth, "kota");
    assert_eq!(generated[0].lemma, "kot");
    assert_eq!(generated[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
    assert_eq!(generated[0].name(engine.resolver()), Some("wlasna"));
    assert_eq!(
        generated[0].labels_as_string(engine.resolver()),
        Some("a|b")
    );
    assert_eq!(unknown[0].tag(engine.resolver()), Some("ign"));
}

#[test]
fn binary_generator_filters_requested_homonym_id() {
    let lexicon =
        BinaryGeneratorLexicon::from_bytes(binary_generator_dictionary_with_homonyms_bytes())
            .unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let all = engine.generate("kot").unwrap();
    let s1 = engine.generate("kot:s1").unwrap();
    let unknown = engine.generate("kot:s3").unwrap();

    assert_eq!(all.len(), 2);
    assert_eq!(all[0].orth, "kota");
    assert_eq!(all[0].lemma, "kot:s1");
    assert_eq!(all[1].orth, "kotu");
    assert_eq!(all[1].lemma, "kot:s2");
    assert_eq!(s1.len(), 1);
    assert_eq!(s1[0].orth, "kota");
    assert_eq!(s1[0].lemma, "kot:s1");
    assert_eq!(unknown.len(), 1);
    assert_eq!(unknown[0].orth, "kot:s3");
    assert_eq!(unknown[0].tag(engine.resolver()), Some("ign"));
}

#[test]
fn binary_analyzer_lexicon_integrates_with_engine_analyze() {
    let lexicon = BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let analyzed = engine.analyze("Kot pies").unwrap();

    assert_eq!(analyzed.len(), 2);
    assert_eq!(analyzed[0].orth, "Kot");
    assert_eq!(analyzed[0].lemma, "kot");
    assert_eq!(analyzed[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
    assert_eq!(analyzed[0].name(engine.resolver()), Some("wlasna"));
    assert_eq!(analyzed[0].labels_as_string(engine.resolver()), Some("a|b"));
    assert_eq!(analyzed[1].orth, "pies");
    assert_eq!(analyzed[1].tag(engine.resolver()), Some("ign"));
}

#[test]
fn word_template_cache_rebases_nodes_after_second_sighting() {
    let lexicon = BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
    let segmentation = SegmentationPreset::default();

    let first = lexicon
        .analyze_native_word(
            "Kot",
            10,
            CaseHandling::ConditionallyCaseSensitive,
            &segmentation,
        )
        .unwrap();
    let second = lexicon
        .analyze_native_word(
            "Kot",
            20,
            CaseHandling::ConditionallyCaseSensitive,
            &segmentation,
        )
        .unwrap();
    let third = lexicon
        .analyze_native_word(
            "Kot",
            30,
            CaseHandling::ConditionallyCaseSensitive,
            &segmentation,
        )
        .unwrap();

    assert_eq!(first.0[0].start_node, 10);
    assert_eq!(second.0[0].start_node, 20);
    assert_eq!(third.0[0].start_node, 30);
    assert_eq!(third.0[0].end_node, 31);
    assert_eq!(third.1, 31);
    assert_eq!(third.0[0].lemma, "kot");
    let stats = lexicon.word_template_cache.stats().unwrap();
    assert_eq!(stats.first_seen, 1);
    assert_eq!(stats.second_seen, 1);
    assert_eq!(stats.inserts, 1);
    assert_eq!(stats.hits, 1);
}

#[test]
fn word_template_cache_does_not_store_ign_only_results() {
    let lexicon = BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
    let segmentation = SegmentationPreset::default();

    for start_node in [0, 10, 20] {
        let (interps, next_node) = lexicon
            .analyze_native_word(
                "pies",
                start_node,
                CaseHandling::ConditionallyCaseSensitive,
                &segmentation,
            )
            .unwrap();
        assert_eq!(next_node, start_node + 1);
        assert!(interps.iter().all(MorphInterpretation::is_ign));
    }

    let stats = lexicon.word_template_cache.stats().unwrap();
    assert_eq!(stats.inserts, 0);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.first_seen, 0);
    assert_eq!(stats.reject_admission, 3);
}

#[test]
fn word_template_cache_respects_case_handling_key() {
    let lexicon =
        BinaryAnalyzerLexicon::from_bytes(binary_titlecase_analyzer_dictionary_bytes()).unwrap();
    let segmentation = SegmentationPreset::default();

    lexicon
        .analyze_native_word(
            "kot",
            0,
            CaseHandling::ConditionallyCaseSensitive,
            &segmentation,
        )
        .unwrap();
    lexicon
        .analyze_native_word(
            "kot",
            1,
            CaseHandling::ConditionallyCaseSensitive,
            &segmentation,
        )
        .unwrap();

    let strict = lexicon
        .analyze_native_word("kot", 2, CaseHandling::StrictlyCaseSensitive, &segmentation)
        .unwrap();

    assert!(strict.0.iter().all(MorphInterpretation::is_ign));
    let stats = lexicon.word_template_cache.stats().unwrap();
    assert_eq!(stats.inserts, 1);
    assert_eq!(stats.hits, 0);
    assert_eq!(stats.first_seen, 1);
    assert_eq!(stats.reject_admission, 1);
}

#[test]
fn word_template_cache_hits_before_append_whitespace_decoration() {
    let lexicon = BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
    let engine = Engine::builder()
        .lexicon(lexicon.clone())
        .config(crate::Config::default().with_whitespace(crate::WhitespaceHandling::Append))
        .build();

    let analyzed = engine.analyze(" Kot Kot Kot ").unwrap();

    assert_eq!(
        analyzed
            .iter()
            .map(|interp| interp.orth.as_str())
            .collect::<Vec<_>>(),
        [" Kot ", "Kot ", "Kot "]
    );
    assert!(analyzed.iter().all(|interp| interp.tag_id == 42));
    let stats = lexicon.word_template_cache.stats().unwrap();
    assert_eq!(stats.inserts, 1);
    assert_eq!(stats.hits, 1);
}

#[test]
fn word_template_cache_preserves_continuous_numbering_with_keep_whitespace() {
    let lexicon = BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
    let engine = Engine::builder()
        .lexicon(lexicon.clone())
        .config(
            crate::Config::default()
                .with_numbering(crate::NumberingScope::Continuous)
                .with_whitespace(crate::WhitespaceHandling::Keep),
        )
        .build();
    let mut session = engine.session();

    let first = session.analyze("Kot Kot").unwrap();
    let second = session.analyze("Kot").unwrap();

    assert_eq!(first[0].start_node, 0);
    assert_eq!(first[1].start_node, 1);
    assert!(first[1].is_whitespace());
    assert_eq!(first[2].start_node, 2);
    assert_eq!(second[0].start_node, 3);
    assert_eq!(second[0].end_node, 4);
    let stats = lexicon.word_template_cache.stats().unwrap();
    assert_eq!(stats.inserts, 1);
    assert_eq!(stats.hits, 1);
}

#[test]
fn binary_analyzer_uses_conditional_case_pattern_fallback() {
    let lexicon =
        BinaryAnalyzerLexicon::from_bytes(binary_titlecase_analyzer_dictionary_bytes()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let title = engine.analyze("Kot").unwrap();
    let lower = engine.analyze("kot").unwrap();

    assert_eq!(title.len(), 1);
    assert_eq!(title[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
    assert_eq!(lower.len(), 1);
    assert_eq!(lower[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
}

#[test]
fn binary_lexicons_expose_encoded_groups() {
    let analyzer = BinaryAnalyzerLexicon::from_bytes(binary_analyzer_dictionary_bytes()).unwrap();
    let generator =
        BinaryGeneratorLexicon::from_bytes(binary_generator_dictionary_bytes()).unwrap();

    let analyzer_groups = analyzer.lookup_encoded_groups("Kot").unwrap().unwrap();
    let generator_groups = generator.synthesize_encoded_groups("kot").unwrap();

    assert_eq!(analyzer_groups[0].segment_type, 4);
    assert_eq!(analyzer_groups[0].interpretations[0].tag_id, 42);
    assert_eq!(generator_groups[0].segment_type, 4);
    assert_eq!(generator_groups[0].interpretations[0].tag_id, 42);
}

#[test]
fn binary_analyzer_uses_default_segmentation_rules_for_word_graph() {
    let lexicon =
        BinaryAnalyzerLexicon::from_bytes(binary_segmented_analyzer_dictionary_bytes()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let analyzed = engine.analyze("ab").unwrap();

    assert_eq!(analyzed.len(), 2);
    assert_eq!(analyzed[0].start_node, 0);
    assert_eq!(analyzed[0].end_node, 1);
    assert_eq!(analyzed[0].orth, "a");
    assert_eq!(analyzed[0].lemma, "a");
    assert_eq!(analyzed[1].start_node, 1);
    assert_eq!(analyzed[1].end_node, 2);
    assert_eq!(analyzed[1].orth, "b");
    assert_eq!(analyzed[1].lemma, "b");
    assert_eq!(analyzed[1].tag(engine.resolver()), Some("subst:sg:nom:m1"));
}

#[test]
fn binary_analyzer_applies_shift_orth_segmentation() {
    let lexicon =
        BinaryAnalyzerLexicon::from_bytes(binary_shifted_analyzer_dictionary_bytes()).unwrap();
    let engine = Engine::builder().lexicon(lexicon).build();

    let analyzed = engine.analyze("ab").unwrap();

    assert_eq!(analyzed.len(), 1);
    assert_eq!(analyzed[0].start_node, 0);
    assert_eq!(analyzed[0].end_node, 1);
    assert_eq!(analyzed[0].orth, "ab");
    assert_eq!(analyzed[0].lemma, "ab");
    assert_eq!(analyzed[0].tag(engine.resolver()), Some("subst:sg:nom:m1"));
}

fn minimal_dictionary_bytes() -> Vec<u8> {
    let fsa = [0xaa, 0xbb];
    let mut metadata = Vec::new();
    metadata.extend_from_slice(b"test-dict\0copyright\0");

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC_NUMBER.to_be_bytes());
    bytes.push(VERSION_NUM);
    bytes.push(2);
    bytes.extend_from_slice(&(fsa.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&fsa);
    bytes.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&metadata);
    bytes.extend_from_slice(&segmentation_metadata_bytes());
    bytes
}

fn dictionary_bytes_with_id_resolver() -> Vec<u8> {
    let fsa = [0xaa, 0xbb];
    dictionary_bytes_with_fsa(&fsa)
}

fn binary_generator_dictionary_bytes() -> Vec<u8> {
    let payload = generator_morph_payload(&[4]);
    let mut fsa = Vec::new();
    fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
    fsa.extend_from_slice(&payload);

    dictionary_bytes_with_fsa(&fsa)
}

fn binary_generator_dictionary_with_homonyms_bytes() -> Vec<u8> {
    let payload = generator_morph_payload_with_homonyms(&[4]);
    let mut fsa = Vec::new();
    fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
    fsa.extend_from_slice(&payload);

    dictionary_bytes_with_fsa(&fsa)
}

fn binary_analyzer_dictionary_bytes() -> Vec<u8> {
    let payload = analyzer_morph_payload(&[4]);
    let mut fsa = Vec::new();
    fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
    fsa.extend_from_slice(&payload);

    dictionary_bytes_with_fsa(&fsa)
}

fn binary_titlecase_analyzer_dictionary_bytes() -> Vec<u8> {
    let payload = analyzer_morph_payload_with_compression(ORTH_ONLY_TITLE | LEMMA_ONLY_LOWER);
    let mut fsa = Vec::new();
    fsa.extend_from_slice(&[b'k', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b'o', V2_LAST_FLAG]);
    fsa.extend_from_slice(&[b't', V2_LAST_FLAG | V2_ACCEPTING_FLAG]);
    fsa.extend_from_slice(&payload);

    dictionary_bytes_with_fsa(&fsa)
}

fn binary_segmented_analyzer_dictionary_bytes() -> Vec<u8> {
    let fsa = one_byte_accepting_fsa(&[
        (b'a', analyzer_morph_payload(&[4])),
        (b'b', analyzer_morph_payload(&[4])),
    ]);
    dictionary_bytes_with_fsa_and_segmentation(&fsa, &two_segment_rules_fsa())
}

fn binary_shifted_analyzer_dictionary_bytes() -> Vec<u8> {
    let fsa = one_byte_accepting_fsa(&[
        (b'a', analyzer_morph_payload(&[4])),
        (b'b', analyzer_morph_payload(&[4])),
    ]);
    dictionary_bytes_with_fsa_and_segmentation(&fsa, &shifted_two_segment_rules_fsa())
}

fn analyzer_groups_payload(segment_types: &[u8]) -> Vec<u8> {
    let encoded_interp = vec![ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER, 0, 0, 0, 42, 7, 0, 9];
    interps_groups_bytes(segment_types, &encoded_interp)
}

fn generator_groups_payload(segment_types: &[u8]) -> Vec<u8> {
    let mut encoded_interp = Vec::new();
    encoded_interp.push(0);
    encoded_interp.push(0);
    encoded_interp.push(0);
    encoded_interp.extend_from_slice(b"a\0");
    encoded_interp.extend_from_slice(&42_u16.to_be_bytes());
    encoded_interp.push(7);
    encoded_interp.extend_from_slice(&9_u16.to_be_bytes());
    interps_groups_bytes(segment_types, &encoded_interp)
}

fn analyzer_morph_payload(segment_types: &[u8]) -> Vec<u8> {
    let encoded_interp = analyzer_interp_with_compression(ORTH_ONLY_LOWER | LEMMA_ONLY_LOWER);
    morph_payload(segment_types, &encoded_interp)
}

fn analyzer_morph_payload_with_compression(compression: u8) -> Vec<u8> {
    let encoded_interp = analyzer_interp_with_compression(compression);
    morph_payload(&[4], &encoded_interp)
}

fn analyzer_interp_with_compression(compression: u8) -> Vec<u8> {
    vec![compression, 0, 0, 0, 42, 7, 0, 9]
}

fn generator_morph_payload(segment_types: &[u8]) -> Vec<u8> {
    let mut encoded_interp = Vec::new();
    encoded_interp.push(0);
    encoded_interp.push(0);
    encoded_interp.push(0);
    encoded_interp.extend_from_slice(b"a\0");
    encoded_interp.extend_from_slice(&42_u16.to_be_bytes());
    encoded_interp.push(7);
    encoded_interp.extend_from_slice(&9_u16.to_be_bytes());
    morph_payload(segment_types, &encoded_interp)
}

fn generator_morph_payload_with_homonyms(segment_types: &[u8]) -> Vec<u8> {
    let mut encoded_interps = Vec::new();
    encoded_interps.extend_from_slice(generator_interp_record("s1", "a", 42).as_slice());
    encoded_interps.extend_from_slice(generator_interp_record("s2", "u", 42).as_slice());
    morph_payload(segment_types, &encoded_interps)
}

fn generator_interp_record(homonym_id: &str, suffix_to_add: &str, tag_id: u16) -> Vec<u8> {
    let mut encoded_interp = Vec::new();
    encoded_interp.extend_from_slice(homonym_id.as_bytes());
    encoded_interp.push(0);
    encoded_interp.push(0);
    encoded_interp.push(0);
    encoded_interp.extend_from_slice(suffix_to_add.as_bytes());
    encoded_interp.push(0);
    encoded_interp.extend_from_slice(&tag_id.to_be_bytes());
    encoded_interp.push(7);
    encoded_interp.extend_from_slice(&9_u16.to_be_bytes());
    encoded_interp
}

fn morph_payload(segment_types: &[u8], encoded_interp: &[u8]) -> Vec<u8> {
    let groups = interps_groups_bytes(segment_types, encoded_interp);
    let mut payload = Vec::new();
    payload.extend_from_slice(&(groups.len() as u16).to_be_bytes());
    payload.extend_from_slice(&groups);
    payload
}

fn interps_groups_bytes(segment_types: &[u8], encoded_interp: &[u8]) -> Vec<u8> {
    let mut payload = Vec::new();
    for segment_type in segment_types {
        payload.push(*segment_type);
        payload.extend_from_slice(&(encoded_interp.len() as u16).to_be_bytes());
        payload.extend_from_slice(encoded_interp);
    }
    payload
}

fn dictionary_bytes_with_fsa(fsa: &[u8]) -> Vec<u8> {
    dictionary_bytes_with_fsa_and_segmentation(fsa, &default_rules_fsa())
}

fn dictionary_bytes_with_fsa_and_segmentation(fsa: &[u8], rules_fsa: &[u8]) -> Vec<u8> {
    let mut metadata = Vec::new();
    metadata.extend_from_slice(b"test-dict\0copyright\0");
    metadata.extend_from_slice(b"test-tagset\0");
    push_id_string_table(&mut metadata, &[(0, "ign"), (42, "subst:sg:nom:m1")]);
    push_id_string_table(&mut metadata, &[(0, "_"), (7, "wlasna")]);
    push_id_string_table(&mut metadata, &[(0, "_"), (9, "a|b")]);

    let mut bytes = Vec::new();
    bytes.extend_from_slice(&MAGIC_NUMBER.to_be_bytes());
    bytes.push(VERSION_NUM);
    bytes.push(2);
    bytes.extend_from_slice(&(fsa.len() as u32).to_be_bytes());
    bytes.extend_from_slice(fsa);
    bytes.extend_from_slice(&(metadata.len() as u32).to_be_bytes());
    bytes.extend_from_slice(&metadata);
    bytes.extend_from_slice(&segmentation_metadata_bytes_with_fsa(rules_fsa));
    bytes
}

fn segmentation_metadata_bytes() -> Vec<u8> {
    segmentation_metadata_bytes_with_fsa(&default_rules_fsa())
}

fn segmentation_metadata_bytes_with_fsa(rules_fsa: &[u8]) -> Vec<u8> {
    let mut data = Vec::new();
    data.extend_from_slice(&2_u16.to_be_bytes());
    data.extend_from_slice(&44_u32.to_be_bytes());
    data.extend_from_slice(&46_u32.to_be_bytes());
    data.push(1);
    push_options_map(&mut data, &[("aggl", "permissive"), ("praet", "split")]);
    data.extend_from_slice(&(rules_fsa.len() as u32).to_be_bytes());
    data.extend_from_slice(rules_fsa);
    push_options_map(&mut data, &[("aggl", "permissive"), ("praet", "split")]);
    data
}

fn default_rules_fsa() -> Vec<u8> {
    vec![1, 0]
}

fn segmentation_rules_fsa_bytes() -> Vec<u8> {
    vec![
        0, 2, 4, 0, 0, 10, 5, 1, 0, 12, 1, 0, 0, 1, 6, 0, 0, 18, 3, 0,
    ]
}

fn two_segment_rules_fsa() -> Vec<u8> {
    vec![0, 1, 4, 0, 0, 6, 0, 1, 4, 0, 0, 12, 1, 0]
}

fn shifted_two_segment_rules_fsa() -> Vec<u8> {
    vec![0, 1, 4, 1, 0, 6, 0, 1, 4, 0, 0, 12, 1, 0]
}

fn one_byte_accepting_fsa(entries: &[(u8, Vec<u8>)]) -> Vec<u8> {
    let transitions_len = entries.len() * 2;
    let mut target_offsets = Vec::with_capacity(entries.len());
    let mut next_target_offset = transitions_len;
    for (_, payload) in entries {
        target_offsets.push(next_target_offset);
        next_target_offset += payload.len() + 2;
    }

    let mut fsa = Vec::new();
    for (index, (label, _)) in entries.iter().enumerate() {
        let transition_offset = index * 2;
        let relative_offset = target_offsets[index] - (transition_offset + 2);
        assert!(relative_offset <= V2_FIRST_BYTE_OFFSET_MASK as usize);
        let mut flags = V2_ACCEPTING_FLAG | relative_offset as u8;
        if index + 1 == entries.len() {
            flags |= V2_LAST_FLAG;
        }
        fsa.push(*label);
        fsa.push(flags);
    }

    for (_, payload) in entries {
        fsa.extend_from_slice(payload);
        fsa.push(0);
        fsa.push(V2_LAST_FLAG);
    }

    fsa
}

fn vlength1_fsa_data(states: &[u8]) -> Vec<u8> {
    let mut data = vec![0; V1_INITIAL_STATE_OFFSET];
    data[V1_INITIAL_STATE_OFFSET - 1] = b'^';
    data.extend_from_slice(states);
    data
}

fn push_options_map(out: &mut Vec<u8>, options: &[(&str, &str)]) {
    out.push(options.len() as u8);
    for (key, value) in options {
        out.extend_from_slice(key.as_bytes());
        out.push(0);
        out.extend_from_slice(value.as_bytes());
        out.push(0);
    }
}

fn options_map(options: &[(&str, &str)]) -> BTreeMap<String, String> {
    options
        .iter()
        .map(|(key, value)| ((*key).to_owned(), (*value).to_owned()))
        .collect()
}

fn push_id_string_table(out: &mut Vec<u8>, entries: &[(u16, &str)]) {
    out.extend_from_slice(&(entries.len() as u16).to_be_bytes());
    for (id, value) in entries {
        out.extend_from_slice(&id.to_be_bytes());
        out.extend_from_slice(value.as_bytes());
        out.push(0);
    }
}
