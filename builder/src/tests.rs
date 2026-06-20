use super::*;
use std::collections::{BTreeMap, BTreeSet};

#[test]
fn reads_dict_id_and_copyright() {
    let metadata = read_metadata_from_str(
        "dict.tab",
        "#!DICT-ID sgjp\n#<COPYRIGHT>\nCopyright line 1\nCopyright line 2\n#</COPYRIGHT>\n",
    )
    .unwrap();

    assert_eq!(
        metadata,
        DictionaryMetadata {
            dict_id: "sgjp".to_owned(),
            copyright: "Copyright line 1\nCopyright line 2\n".to_owned(),
        }
    );
}

#[test]
fn defaults_missing_metadata_to_empty_strings() {
    let metadata = read_metadata_from_str("dict.tab", "kot\tkot\tsubst\n").unwrap();

    assert_eq!(metadata.dict_id, "");
    assert_eq!(metadata.copyright, "");
}

#[test]
fn keeps_first_dict_id_across_inputs() {
    let metadata = merge_metadata([
        ("first.tab", "#!DICT-ID first\n"),
        ("second.tab", "#!DICT-ID second\n"),
    ])
    .unwrap();

    assert_eq!(metadata.dict_id, "first");
}

#[test]
fn rejects_dict_id_without_value() {
    let error = read_metadata_from_str("dict.tab", "#!DICT-ID\n").unwrap_err();

    assert_eq!(error.to_string(), "dict.tab:1: Must provide DICT-ID");
}

#[test]
fn rejects_dict_id_tag_without_space_separator() {
    let error = read_metadata_from_str("dict.tab", "#!DICT-ID\tmain\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "Dictionary ID tag must be followed by a space character and dictionary ID string"
    );
}

#[test]
fn accepts_legacy_empty_dict_id_after_space() {
    let metadata = read_metadata_from_str("dict.tab", "#!DICT-ID \n").unwrap();

    assert_eq!(metadata.dict_id, "");
}

#[test]
fn rejects_dict_id_containing_spaces() {
    let error = read_metadata_from_str("dict.tab", "#!DICT-ID sgjp main\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "dict.tab:1: DICT-ID must not contain spaces"
    );
}

#[test]
fn rejects_copyright_start_with_extra_text() {
    let error = read_metadata_from_str("dict.tab", "#<COPYRIGHT> extra\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "dict.tab:1: Copyright start tag must be the only one in the line"
    );
}

#[test]
fn rejects_copyright_end_without_start() {
    let error = read_metadata_from_str("dict.tab", "#</COPYRIGHT>\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "dict.tab:1: Copyright end tag must be preceded by copyright start tag"
    );
}

#[test]
fn rejects_copyright_end_with_extra_text() {
    let error = read_metadata_from_str("dict.tab", "#<COPYRIGHT>\ntext\n#</COPYRIGHT> extra\n")
        .unwrap_err();

    assert_eq!(
        error.to_string(),
        "dict.tab:3: Copyright end tag must be the only one in the line"
    );
}

#[test]
fn parses_tagset_with_legacy_tag_indexes_and_insertion_order() {
    let tagset = Tagset::from_str(
        "sample.tagset",
        "#!TAGSET-ID   sgjp\n# comment\n\n[TAGS]\n2\tsubst\n0\tadj\n",
    )
    .unwrap();

    assert_eq!(tagset.tagset_id.as_deref(), Some("sgjp"));
    assert_eq!(tagset.all_tags(), &["subst".to_owned(), "adj".to_owned()]);
    assert_eq!(tagset.tag_num_for_tag("subst").unwrap(), 2);
    assert_eq!(tagset.tag_for_tag_num(0).unwrap(), "adj");
    assert_eq!(TagsetLookup::tag_num(&tagset, "adj").unwrap(), 0);
}

#[test]
fn parses_empty_tagset_id_like_python_regex() {
    let tagset = Tagset::from_str("sample.tagset", "#!TAGSET-ID   \n[TAGS]\n").unwrap();

    assert_eq!(tagset.tagset_id.as_deref(), Some(""));
}

#[test]
fn rejects_missing_tagset_id_in_first_line() {
    let error =
        Tagset::from_str("sample.tagset", "#!MORFEUSZ-TAGSET 0.1\n[TAGS]\n0\ttag\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "missing TAGSET-ID in first line of tagset file"
    );
}

#[test]
fn rejects_text_outside_tags_section() {
    let error = Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n0\ttag\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "\"0\ttag\" - text outside [TAGS] section in tagset file line 2"
    );
}

#[test]
fn rejects_invalid_tagset_line_shape() {
    let error =
        Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\textra\n").unwrap_err();

    assert_eq!(error.to_string(), "\"0\ttag\textra\" - invalid line 3");
}

#[test]
fn rejects_duplicate_tag() {
    let error =
        Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\n1\ttag\n").unwrap_err();

    assert_eq!(error.to_string(), "duplicate tag: \"tag\"");
}

#[test]
fn rejects_duplicate_tag_id() {
    let error =
        Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\n0\tother\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "line 4: tagId 0 assigned for tag \"other\" already appeared somewhere else."
    );
}

#[test]
fn rejects_invalid_tag_lookup() {
    let tagset = Tagset::from_str("sample.tagset", "#!TAGSET-ID x\n[TAGS]\n0\ttag\n").unwrap();

    assert_eq!(
        tagset.tag_num_for_tag("missing").unwrap_err().to_string(),
        "invalid tag: \"missing\""
    );
    assert_eq!(
        tagset.tag_for_tag_num(9).unwrap_err().to_string(),
        "invalid tag id: 9"
    );
}

#[test]
fn serializes_legacy_numbers_strings_and_prologue() {
    assert_eq!(serialize_u16_be(0x1234).unwrap(), [0x12, 0x34]);
    assert_eq!(
        serialize_u32_be(0x1234_5678).unwrap(),
        [0x12, 0x34, 0x56, 0x78]
    );
    assert_eq!(serialize_legacy_string("zazolc"), b"zazolc\0");
    assert_eq!(serialize_prologue(2), vec![0x8f, 0xc2, 0xbc, 0x1b, 21, 2]);
}

#[test]
fn rejects_legacy_number_overflow() {
    assert_eq!(
        serialize_u16_be(65_536).unwrap_err().to_string(),
        "value 65536 does not fit into uint16"
    );
    assert_eq!(
        serialize_u32_be(u32::MAX as usize + 1)
            .unwrap_err()
            .to_string(),
        "value 4294967296 does not fit into uint32"
    );
}

#[test]
fn serializes_tags_map_like_legacy_serializer() {
    let tags = BTreeMap::from([("b".to_owned(), 2), ("a".to_owned(), 1)]);

    assert_eq!(
        hex(&serialize_tags_map(&tags, &Utf8WordEncoder).unwrap()),
        "00020001610000026200"
    );
}

#[test]
fn serializes_qualifiers_map_like_legacy_serializer() {
    let qualifiers_map = BTreeMap::from([
        (qualifiers([]), 0),
        (qualifiers(["x"]), 1),
        (qualifiers(["arch", "rare"]), 2),
    ]);

    assert_eq!(
        hex(&serialize_qualifiers_map(&qualifiers_map, &Utf8WordEncoder).unwrap()),
        "0003000000000178000002617263687c7261726500"
    );
}

#[test]
fn serializes_tagset_data_like_legacy_serializer() {
    let tagset =
        Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n2\tb\n1\ta\n").unwrap();
    let names = BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)]);

    assert_eq!(
        hex(&serialize_tagset_data(&tagset, &names, &Utf8WordEncoder).unwrap()),
        "7469640000020001610000026200000200000000016e616d6500"
    );
}

#[test]
fn serializes_epilogue_like_legacy_serializer() {
    let tagset =
        Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n2\tb\n1\ta\n").unwrap();
    let names = BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)]);
    let tagset_data = serialize_tagset_data(&tagset, &names, &Utf8WordEncoder).unwrap();
    let qualifiers_map = BTreeMap::from([
        (qualifiers([]), 0),
        (qualifiers(["x"]), 1),
        (qualifiers(["arch", "rare"]), 2),
    ]);
    let qualifiers_data = serialize_qualifiers_map(&qualifiers_map, &Utf8WordEncoder).unwrap();

    assert_eq!(
            hex(&serialize_epilogue(
                "dict",
                "copy",
                &tagset_data,
                &qualifiers_data,
                &[1, 2, 3],
            )
            .unwrap()),
            "000000396469637400636f7079007469640000020001610000026200000200000000016e616d65000003000000000178000002617263687c7261726500010203"
        );
}

#[test]
fn serializes_simple_state_like_legacy_serializer() {
    let state = simple_oracle_state();
    let global = simple_global_frequencies();

    assert_eq!(simple_implementation_code(false), 0);
    assert_eq!(simple_state_size(&state, false).unwrap(), 15);
    assert_eq!(hex(&serialize_simple_state_data(&state).unwrap()), "83aabb");
    assert_eq!(
        hex(&serialize_simple_transitions(&state, false, &global).unwrap()),
        "620001026101020363000001"
    );
    assert_eq!(
        hex(&serialize_simple_state(&state, false, &global).unwrap()),
        "83aabb620001026101020363000001"
    );
}

#[test]
fn serializes_simple_state_with_transition_data_like_legacy_serializer() {
    let state = simple_oracle_state();
    let global = simple_global_frequencies();

    assert_eq!(simple_implementation_code(true), 128);
    assert_eq!(simple_state_size(&state, true).unwrap(), 18);
    assert_eq!(
        hex(&serialize_simple_transitions(&state, true, &global).unwrap()),
        "620001020861010203096300000107"
    );
    assert_eq!(
        hex(&serialize_simple_state(&state, true, &global).unwrap()),
        "83aabb620001020861010203096300000107"
    );
}

#[test]
fn serializes_non_accepting_simple_state_with_transition() {
    let state = SimpleState::non_accepting().with_transition(b'x', 0x00000a, None);

    assert_eq!(
        hex(&serialize_simple_state(&state, false, &BTreeMap::new()).unwrap()),
        "017800000a"
    );
}

#[test]
fn rejects_invalid_simple_states_and_offsets() {
    assert_eq!(
        serialize_simple_state_data(&SimpleState::non_accepting())
            .unwrap_err()
            .to_string(),
        "simple state must be accepting or have transitions"
    );

    let too_many = (0..128).fold(SimpleState::non_accepting(), |state, index| {
        state.with_transition(index as u8, 0, None)
    });
    assert_eq!(
        serialize_simple_state_data(&too_many)
            .unwrap_err()
            .to_string(),
        "simple state has too many transitions: 128"
    );

    let too_large_offset =
        SimpleState::non_accepting().with_transition(b'a', 256 * 256 * 256, None);
    assert_eq!(
        serialize_simple_transitions(&too_large_offset, false, &BTreeMap::new())
            .unwrap_err()
            .to_string(),
        "simple transition offset 16777216 exceeds 24-bit limit"
    );

    let missing_data = SimpleState::non_accepting().with_transition(b'a', 1, None);
    assert_eq!(
        serialize_simple_transitions(&missing_data, true, &BTreeMap::new())
            .unwrap_err()
            .to_string(),
        "missing transition data for label 97"
    );
}

#[test]
fn calculates_simple_graph_offsets_like_legacy_state_dfs() {
    let graph = simple_oracle_graph(false);
    let layout = calculate_simple_graph_layout(&graph, false).unwrap();

    assert_eq!(layout.dfs_order, vec![3, 2, 1, 0]);
    assert_eq!(layout.offsets, vec![0, 9, 14, 19]);
    assert_eq!(layout.reverse_offsets, vec![22, 13, 8, 3]);
    assert_eq!(layout.total_size, 22);
}

#[test]
fn serializes_simple_fsa_data_like_legacy_serializer() {
    let graph = simple_oracle_graph(false);

    assert_eq!(
        hex(&serialize_simple_fsa_data(&graph, false).unwrap()),
        "02610000096200000e0178000013016300001380dead"
    );
}

#[test]
fn serializes_simple_fsa_data_with_transition_data_like_legacy_serializer() {
    let graph = simple_oracle_graph(true);
    let layout = calculate_simple_graph_layout(&graph, true).unwrap();

    assert_eq!(layout.dfs_order, vec![3, 2, 1, 0]);
    assert_eq!(layout.offsets, vec![0, 11, 17, 23]);
    assert_eq!(layout.reverse_offsets, vec![26, 15, 9, 3]);
    assert_eq!(layout.total_size, 26);
    assert_eq!(
        hex(&serialize_simple_fsa_data(&graph, true).unwrap()),
        "026100000b09620000110801780000170301630000170480dead"
    );
}

#[test]
fn serializes_full_simple_dictionary_like_legacy_serializer() {
    let (tagset, names, qualifiers) = simple_dictionary_metadata();

    assert_eq!(
            hex(&serialize_simple_dictionary(
                &simple_oracle_graph(false),
                false,
                "dict",
                "copy",
                &tagset,
                &names,
                &qualifiers,
                &[1, 2, 3],
                &Utf8WordEncoder,
            )
            .unwrap()),
            "8fc2bc1b15000000001602610000096200000e0178000013016300001380dead000000396469637400636f7079007469640000020001610000026200000200000000016e616d65000003000000000178000002617263687c7261726500010203"
        );
}

#[test]
fn serializes_full_simple_dictionary_with_transition_data_like_legacy_serializer() {
    let (tagset, names, qualifiers) = simple_dictionary_metadata();

    assert_eq!(
            hex(&serialize_simple_dictionary(
                &simple_oracle_graph(true),
                true,
                "dict",
                "copy",
                &tagset,
                &names,
                &qualifiers,
                &[1, 2, 3],
                &Utf8WordEncoder,
            )
            .unwrap()),
            "8fc2bc1b15800000001a026100000b09620000110801780000170301630000170480dead000000396469637400636f7079007469640000020001610000026200000200000000016e616d65000003000000000178000002617263687c7261726500010203"
        );
}

#[test]
fn builds_minimized_simple_fsa_from_sorted_entries_like_legacy_builder() {
    let graph = build_simple_fsa_from_sorted_entries(constructed_simple_entries()).unwrap();

    assert_eq!(
        graph.global_label_frequencies,
        BTreeMap::from([(b'a', 2), (b'b', 4)])
    );
    assert_eq!(
        hex(&serialize_simple_fsa_data(&graph, false).unwrap()),
        "02620000096100000f8103620000158101620000158002"
    );
}

#[test]
fn builds_full_simple_dictionary_from_sorted_entries_like_legacy_builder() {
    let graph = build_simple_fsa_from_sorted_entries(constructed_simple_entries()).unwrap();
    let tagset = Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n0\ttag\n").unwrap();
    let names = BTreeMap::from([(String::new(), 0)]);
    let qualifiers = BTreeMap::from([(qualifiers([]), 0)]);

    assert_eq!(
            hex(&serialize_simple_dictionary(
                &graph,
                false,
                "dict",
                "copy",
                &tagset,
                &names,
                &qualifiers,
                &[],
                &Utf8WordEncoder,
            )
            .unwrap()),
            "8fc2bc1b15000000001702620000096100000f8103620000158101620000158002000000206469637400636f70790074696400000100007461670000010000000001000000"
        );
}

#[test]
fn rejects_invalid_sorted_fsa_inputs_like_legacy_assertions() {
    assert_eq!(
        build_simple_fsa_from_sorted_entries(Vec::<(Vec<u8>, Vec<u8>)>::new())
            .unwrap_err()
            .to_string(),
        "empty input"
    );
    assert_eq!(
        build_simple_fsa_from_sorted_entries(vec![(Vec::new(), vec![1])])
            .unwrap_err()
            .to_string(),
        "entry word must not be empty"
    );
    assert_eq!(
        build_simple_fsa_from_sorted_entries(vec![
            (b"b".to_vec(), vec![1]),
            (b"a".to_vec(), vec![2]),
        ])
        .unwrap_err()
        .to_string(),
        "input entries must be strictly sorted by encoded word"
    );
    assert_eq!(
        build_simple_fsa_from_sorted_entries(vec![
            (b"a".to_vec(), vec![1]),
            (b"a".to_vec(), vec![2]),
        ])
        .unwrap_err()
        .to_string(),
        "input entries must be strictly sorted by encoded word"
    );
}

#[test]
fn serializes_analyzer_entry_payload_like_legacy_morph_encoder() {
    let entry = AnalyzerEntry {
        key: "kot".to_owned(),
        interpretations: vec![
            AnalyzerInterpretation::new("Kot", "kot", 11, 1, 1, 4).unwrap(),
            AnalyzerInterpretation::new("Kot", "Kot", 10, 1, 2, 3).unwrap(),
        ],
    };

    assert_eq!(
        hex(&serialize_analyzer_entry_payload(&entry).unwrap()),
        "0016010008600000000b010004020008500000000a010003"
    );
}

#[test]
fn serializes_analyzer_mixed_case_payload_like_legacy_morph_encoder() {
    let entry = AnalyzerEntry {
        key: "abcde".to_owned(),
        interpretations: vec![
            AnalyzerInterpretation::new("AbCde", "Xy", 513, 2, 7, 9).unwrap(),
            AnalyzerInterpretation::new("AbCde", "ABcxy", 514, 3, 7, 10).unwrap(),
        ],
    };

    assert_eq!(
        hex(&serialize_analyzer_entry_payload(&entry).unwrap()),
        "001f07001c000002020002027879000102020203000a0005587900000201020009"
    );
}

#[test]
fn serializes_generator_entry_payload_like_legacy_generator_encoder() {
    let entry = GeneratorEntry {
        key: "kot".to_owned(),
        interpretations: vec![
            GeneratorInterpretation::new("przedkotami", "kot", 513, 2, 7, "h", 9).unwrap(),
            GeneratorInterpretation::new("koty", "kot", 514, 3, 7, "", 10).unwrap(),
        ],
    };

    assert_eq!(
        hex(&serialize_generator_entry_payload(&entry).unwrap()),
        "002207001f0000007900020203000a6800000370727a65646b6f74616d69000201020009"
    );
}

#[test]
fn converts_analyzer_entries_to_sorted_simple_fsa_entries() {
    let entries = vec![
        AnalyzerEntry {
            key: "a".to_owned(),
            interpretations: vec![AnalyzerInterpretation::new("a", "a", 1, 0, 1, 0).unwrap()],
        },
        AnalyzerEntry {
            key: "b".to_owned(),
            interpretations: vec![AnalyzerInterpretation::new("b", "b", 2, 0, 1, 0).unwrap()],
        },
    ];

    let fsa_entries = analyzer_entries_to_sorted_fsa_entries(&entries, &Utf8WordEncoder).unwrap();
    assert_eq!(fsa_entries[0].0, b"a");
    assert_eq!(hex(&fsa_entries[0].1), "000b010008a000000001000000");
    assert_eq!(fsa_entries[1].0, b"b");

    let graph = build_analyzer_simple_fsa_from_entries(&entries, &Utf8WordEncoder).unwrap();
    assert_eq!(
        graph.global_label_frequencies,
        BTreeMap::from([(b'a', 1), (b'b', 1)])
    );
}

#[test]
fn builds_simple_dictionary_from_analyzer_entries() {
    let entries = vec![AnalyzerEntry {
        key: "a".to_owned(),
        interpretations: vec![AnalyzerInterpretation::new("a", "a", 1, 0, 1, 0).unwrap()],
    }];
    let tagset = Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n1\ttag\n").unwrap();
    let names = BTreeMap::from([(String::new(), 0)]);
    let qualifiers = BTreeMap::from([(qualifiers([]), 0)]);

    let bytes = build_analyzer_simple_dictionary_from_entries(
        &entries,
        "dict",
        "copy",
        &tagset,
        &names,
        &qualifiers,
        &[1, 2, 3],
        &Utf8WordEncoder,
    )
    .unwrap();

    assert!(bytes.starts_with(&[0x8f, 0xc2, 0xbc, 0x1b, DICTIONARY_VERSION, 0x00]));
    assert!(hex(&bytes).contains("6469637400636f70790074696400"));
}

#[test]
fn builds_simple_dictionaries_from_source_strings() {
    let dictionary = "#!DICT-ID dict\n#<COPYRIGHT>\ncopy\n#</COPYRIGHT>\nKot\tkot\ttag\tname\tq\n";
    let tagset = "#!TAGSET-ID tid\n[TAGS]\n0\tign\n1\tsp\n10\ttag\n";
    let segmentation = "[options]\n\
aggl = isolated\n\
praet = split\n\
[combinations]\n\
A\n\
[tags]\n\
A %\n\
[lexemes]\n\
[segment types]\n\
A\n\
[separator chars]\n";

    let analyzer = build_analyzer_simple_dictionary_from_str(
        "dict.tab",
        dictionary,
        "tagset.dat",
        tagset,
        "segmenty.dat",
        segmentation,
    )
    .unwrap();
    let generator = build_generator_simple_dictionary_from_str(
        "dict.tab",
        dictionary,
        "tagset.dat",
        tagset,
        "segmenty.dat",
        segmentation,
    )
    .unwrap();

    assert!(analyzer.starts_with(&[0x8f, 0xc2, 0xbc, 0x1b, DICTIONARY_VERSION, 0x00]));
    assert!(generator.starts_with(&[0x8f, 0xc2, 0xbc, 0x1b, DICTIONARY_VERSION, 0x00]));
    assert!(hex(&analyzer).contains("6469637400636f70790a0074696400"));
    assert!(hex(&generator).contains("6469637400636f70790a0074696400"));
}

#[test]
fn rejects_empty_source_dictionary_builds() {
    let error = build_analyzer_simple_dictionary_from_sources(
        std::iter::empty(),
        "tagset.dat",
        "",
        "segmenty.dat",
        "",
    )
    .unwrap_err();

    assert_eq!(error.to_string(), "dictionary sources must not be empty");
}

#[test]
fn rejects_invalid_simple_graph_references() {
    let invalid_initial = SimpleFsaGraph {
        states: vec![SimpleGraphState::accepting([1])],
        initial_state: 3,
        global_label_frequencies: BTreeMap::new(),
    };
    assert_eq!(
        calculate_simple_graph_layout(&invalid_initial, false)
            .unwrap_err()
            .to_string(),
        "invalid initial state index: 3"
    );

    let invalid_target = SimpleFsaGraph {
        states: vec![SimpleGraphState::non_accepting().with_transition(b'a', 7, None)],
        initial_state: 0,
        global_label_frequencies: BTreeMap::new(),
    };
    assert_eq!(
        calculate_simple_graph_layout(&invalid_target, false)
            .unwrap_err()
            .to_string(),
        "state 0 transition 97 targets invalid state 7"
    );
}

#[test]
fn reads_names_and_qualifiers_with_legacy_indexes() {
    let result = read_names_and_qualifiers_from_str(
        "dict.tab",
        "pies\tpies\tsubst\nkot\tkot\tsubst\tpospolita\nala\tala\tsubst\twlasna\trare|archaic\n",
    )
    .unwrap();

    assert_eq!(result.names.get(""), Some(&0));
    assert_eq!(result.names.get("pospolita"), Some(&1));
    assert_eq!(result.names.get("wlasna"), Some(&2));

    assert_eq!(result.qualifiers.get(&qualifiers([])), Some(&0));
    assert_eq!(
        result.qualifiers.get(&qualifiers(["archaic", "rare"])),
        Some(&1)
    );
}

#[test]
fn ignores_metadata_copyright_and_space_containing_forms() {
    let result = read_names_and_qualifiers_from_str(
            "dict.tab",
            "#!DICT-ID sgjp\n#<COPYRIGHT>\ninside\tinside\ttag\tignored\tq1\n#</COPYRIGHT>\nbad orth\tbad\ttag\tignored\tq2\ngood\tbad lemma\ttag\tignored\tq3\ngood\tgood\ttag\tkept\tq4\n",
        )
        .unwrap();

    assert_eq!(result.names.len(), 2);
    assert_eq!(result.names.get(""), Some(&0));
    assert_eq!(result.names.get("kept"), Some(&1));
    assert_eq!(result.qualifiers.len(), 2);
    assert_eq!(result.qualifiers.get(&qualifiers([])), Some(&0));
    assert_eq!(result.qualifiers.get(&qualifiers(["q4"])), Some(&1));
}

#[test]
fn parses_three_four_and_five_field_lines() {
    let result = read_names_and_qualifiers_from_str(
        "dict.tab",
        "a\ta\ttag\nb\tb\ttag\tname\nc\tc\ttag\tname2\tq\n",
    )
    .unwrap();

    assert_eq!(result.names.get(""), Some(&0));
    assert_eq!(result.names.get("name"), Some(&1));
    assert_eq!(result.names.get("name2"), Some(&2));
    assert_eq!(result.qualifiers.get(&qualifiers([])), Some(&0));
    assert_eq!(result.qualifiers.get(&qualifiers(["q"])), Some(&1));
}

#[test]
fn rejects_invalid_tab_field_count() {
    let error = read_names_and_qualifiers_from_str("dict.tab", "a\tb\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "input line \"a\tb\" does not have 3, 4 or 5 tab-separated fields"
    );
}

#[test]
fn malformed_dict_id_without_space_flows_to_line_parser_like_legacy() {
    let error = read_names_and_qualifiers_from_str("dict.tab", "#!DICT-ID\n").unwrap_err();

    assert_eq!(
        error.to_string(),
        "input line \"#!DICT-ID\" does not have 3, 4 or 5 tab-separated fields"
    );
}

#[test]
fn parse_qualifiers_keeps_empty_members() {
    assert_eq!(parse_qualifiers("rare|"), qualifiers(["", "rare"]));
}

#[test]
fn preprocesses_segment_rule_defines_like_legacy_preprocessor() {
    assert_eq!(
        replace_ascii_word(" left x right", "x", "A B"),
        " left A B right"
    );

    let lines = [
        (1, "#define A x"),
        (2, "A B"),
        (3, "#define WRAP(x) left x right"),
        (4, "WRAP(A B)"),
        (5, "#define B(y) A y"),
        (6, "B(C)"),
    ];

    assert_eq!(
        preprocess_segment_rules(lines, std::iter::empty::<&str>(), "segmenty.dat").unwrap(),
        vec![
            (2, "x B".to_owned()),
            (4, " left x B right".to_owned()),
            (6, "x C".to_owned()),
        ]
    );
}

#[test]
fn preprocesses_segment_rule_conditionals_like_legacy_preprocessor() {
    let lines = [
        (1, "#ifdef extra"),
        (2, "A"),
        (3, "#else"),
        (4, "B"),
        (5, "#endif"),
    ];

    assert_eq!(
        preprocess_segment_rules(lines, ["extra"], "segmenty.dat").unwrap(),
        vec![(2, "A".to_owned())]
    );
    assert_eq!(
        preprocess_segment_rules(lines, std::iter::empty::<&str>(), "segmenty.dat").unwrap(),
        vec![(4, "B".to_owned())]
    );
}

#[test]
fn preprocesses_segment_rule_calls_comments_and_operators_like_legacy_preprocessor() {
    let lines = [
        (1, "#define A x y"),
        (2, "A(B)"),
        (3, "UNKNOWN(B C)"),
        (4, "# comment"),
        (5, "#define SHIFT x>"),
        (6, "(SHIFT | B)+ !weak"),
    ];

    assert_eq!(
        preprocess_segment_rules(lines, std::iter::empty::<&str>(), "segmenty.dat").unwrap(),
        vec![
            (2, "x y ( B )".to_owned()),
            (3, "UNKNOWN ( B C )".to_owned()),
            (4, "# comment".to_owned()),
            (6, "( x> | B ) + !weak".to_owned()),
        ]
    );
}

#[test]
fn parses_segment_rule_lines_like_legacy_parser() {
    for (line, expected, weak, empty, shift_orth) in [
        ("A", "A", false, false, false),
        ("A>", "A>", false, false, true),
        ("(A)>", "A>", false, false, true),
        ("A B", "A B", false, false, false),
        ("A | B", "A | B", false, false, false),
        ("A|B C", "A | B C", false, false, false),
        ("A B|C", "A B | C", false, false, false),
        ("(A | B) C", "A | B C", false, false, false),
        ("A*", "(A)*", false, true, false),
        ("A+", "A (A)*", false, false, false),
        ("A?", "(A)?", false, true, false),
        ("A{2}", "A A", false, false, false),
        ("A{0,2}", "(A)? | A A", false, true, false),
        ("A{2,4}", "A A | A A A | A A A A", false, false, false),
        ("A{2,}", "A A (A)*", false, false, false),
        ("A> B>", "A> B>", false, false, true),
        ("A !weak", "A", true, false, false),
        ("(A B)>", "A> B>", false, false, true),
    ] {
        let rule = parsed_segment_rule(line);
        assert_eq!(rule.to_string(), expected, "{line}");
        assert_eq!(rule.is_weak(), weak, "{line}");
        assert_eq!(rule.allows_empty_sequence(), empty, "{line}");
        assert_eq!(rule.is_shift_orth_rule(), shift_orth, "{line}");
        rule.validate_segment_rule("<case>")
            .unwrap_or_else(|err| panic!("{line} should validate like legacy parser, got {err}"));
    }
}

#[test]
fn transforms_segment_rules_to_generator_like_legacy_parser() {
    for (line, expected_generator, expected_additional) in [
        ("A", "A", vec!["A"]),
        ("A>", "A>", vec!["A>"]),
        ("(A)>", "A>", vec!["A>"]),
        ("A B", "<<REMOVED>>", vec!["A", "B"]),
        ("A | B", "A | B", vec!["A", "B"]),
        ("A|B C", "<<REMOVED>>", vec!["A", "B", "C"]),
        ("A B|C", "<<REMOVED>>", vec!["A", "B", "C"]),
        ("(A | B) C", "<<REMOVED>>", vec!["A", "B", "C"]),
        ("A*", "<<REMOVED>>", vec!["A"]),
        ("A+", "A", vec!["A", "A"]),
        ("A?", "A", vec!["A"]),
        ("A{2}", "<<REMOVED>>", vec!["A", "A"]),
        ("A{0,2}", "<<REMOVED>>", vec!["A", "A", "A"]),
        (
            "A{2,4}",
            "<<REMOVED>>",
            vec!["A", "A", "A", "A", "A", "A", "A", "A", "A"],
        ),
        ("A{2,}", "<<REMOVED>>", vec!["A", "A", "A"]),
        ("A> B>", "A> B>", vec![]),
        ("A B>", "<<REMOVED>>", vec!["A"]),
        ("A> | B>", "A> | B>", vec!["A>", "B>"]),
        ("A> | B", "A> | B", vec!["A>", "B"]),
        ("A !weak", "A", vec!["A"]),
        ("(A B)>", "A> B>", vec![]),
    ] {
        let rule = parsed_segment_rule(line);
        assert_eq!(
            rule.transform_to_generator_version().to_string(),
            expected_generator,
            "{line}"
        );
        assert_eq!(
            segment_rule_strings(rule.additional_atomic_rules_for_generator()),
            expected_additional,
            "{line}"
        );
    }
}

#[test]
fn validates_segment_rule_shift_orth_constraints_like_legacy() {
    let error = parsed_segment_rule("A B>")
        .validate_segment_rule("<case>")
        .unwrap_err();
    assert_eq!(
            error.to_string(),
            "<case>:7 - If the rightmost subrule of concatenation \"A B>\" is with \">\", than all subrules must be with \">\""
        );

    let error = parsed_segment_rule("A> | B")
        .validate_segment_rule("<case>")
        .unwrap_err();
    assert_eq!(
        error.to_string(),
        "<case>:7 - All subrules of alternative \"A> | B\" must be either with or without \">\""
    );

    parsed_segment_rule("A> B>")
        .validate_segment_rule("<case>")
        .unwrap();
}

#[test]
fn rejects_invalid_segment_rule_quantities_and_unknown_segment_types() {
    assert_eq!(
        parse_segment_rule_line(7, "A{0}", &segment_types(), "<case>")
            .unwrap_err()
            .to_string(),
        "<case>:7: A{0} - invalid quantity: 0"
    );
    assert_eq!(
        parse_segment_rule_line(7, "A{3,2}", &segment_types(), "<case>")
            .unwrap_err()
            .to_string(),
        "<case>:7: A{3,2} - invalid quantities: 3 2"
    );
    assert_eq!(
        parse_segment_rule_line(7, "NOPE", &segment_types(), "<case>")
            .unwrap_err()
            .to_string(),
        "<case>:7: unknown segment type: NOPE"
    );
}

#[test]
fn serializes_segment_rules_fsa_like_legacy_python_builder() {
    for (lines, expected_bytes) in [
        (vec!["A"], vec![0, 1, 1, 0, 0, 6, 1, 0]),
        (vec!["A B"], vec![0, 1, 1, 0, 0, 6, 0, 1, 2, 0, 0, 12, 1, 0]),
        (
            vec!["A | B"],
            vec![0, 2, 1, 0, 0, 12, 2, 0, 0, 10, 1, 0, 1, 0],
        ),
        (
            vec!["A? B"],
            vec![0, 2, 1, 0, 0, 10, 2, 0, 0, 16, 0, 1, 2, 0, 0, 16, 1, 0],
        ),
        (
            vec!["A* B"],
            vec![
                0, 2, 1, 0, 0, 10, 2, 0, 0, 20, 0, 2, 1, 0, 0, 10, 2, 0, 0, 20, 1, 0,
            ],
        ),
        (vec!["A>"], vec![0, 1, 1, 1, 0, 6, 1, 0]),
        (vec!["A !weak"], vec![0, 1, 1, 0, 0, 6, 3, 0]),
        (
            vec!["A", "B"],
            vec![0, 2, 1, 0, 0, 12, 2, 0, 0, 10, 1, 0, 1, 0],
        ),
        (
            vec!["A B", "A C"],
            vec![0, 1, 1, 0, 0, 6, 0, 2, 2, 0, 0, 16, 3, 0, 0, 18, 1, 0, 1, 0],
        ),
    ] {
        let rules = parsed_segment_rules(&lines);
        assert_eq!(
            serialize_segment_rules_fsa(rules.iter(), "<case>").unwrap(),
            expected_bytes,
            "{lines:?}"
        );
    }
}

#[test]
fn rejects_segment_rules_fsa_weakness_conflicts_like_legacy_builder() {
    let rules = [
        parsed_segment_rule_at(7, "A B"),
        parsed_segment_rule_at(8, "A B !weak"),
    ];
    let error = serialize_segment_rules_fsa(rules.iter(), "<case>").unwrap_err();

    assert_eq!(
            error.to_string(),
            "<case>:8 - conflicts with rule at line 7. Segmentation for some chunks can be both weak and non-weak which is illegal."
        );
}

#[test]
fn serializes_segmentation_rules_metadata_like_legacy_rules_manager() {
    let variants = vec![
        SegmentRulesFsaVariantData {
            options: segment_rule_options("strict", "split"),
            fsa: vec![0, 1, 2, 3],
        },
        SegmentRulesFsaVariantData {
            options: segment_rule_options("permissive", "composite"),
            fsa: vec![4, 5],
        },
    ];

    assert_eq!(
        serialize_segmentation_rules_data(
            [32, 9],
            &variants,
            &segment_rule_options("strict", "split"),
        )
        .unwrap(),
        vec![
            0, 2, 0, 0, 0, 9, 0, 0, 0, 32, 2, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116,
            0, 112, 114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 4, 0, 1, 2, 3, 2,
            97, 103, 103, 108, 0, 112, 101, 114, 109, 105, 115, 115, 105, 118, 101, 0, 112, 114,
            97, 101, 116, 0, 99, 111, 109, 112, 111, 115, 105, 116, 101, 0, 0, 0, 0, 2, 4, 5, 2,
            97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97, 101, 116, 0, 115,
            112, 108, 105, 116, 0,
        ]
    );
}

#[test]
fn rejects_invalid_segmentation_rules_metadata_options() {
    assert_eq!(
        serialize_segmentation_rules_data(
            [],
            &[SegmentRulesFsaVariantData {
                options: BTreeMap::new(),
                fsa: vec![0, 0],
            }],
            &segment_rule_options("strict", "split"),
        )
        .unwrap_err()
        .to_string(),
        "segmentation options missing aggl"
    );
    assert_eq!(
        serialize_segmentation_rules_data([], &[], &segment_rule_options("strict", "split"),)
            .unwrap_err()
            .to_string(),
        "Too many segmentation rules variants"
    );
}

#[test]
fn parses_analyzer_segmentation_rules_config_like_legacy_pipeline() {
    let parsed = parse_segmentation_rules_from_str(
        "segmenty.dat",
        sample_segment_rules_config(),
        SegmentRulesTarget::Analyzer,
    )
    .unwrap();

    assert_eq!(
        parsed.segmentation_rules_data,
        vec![
            0, 2, 0, 0, 0, 9, 0, 0, 0, 32, 2, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116,
            0, 112, 114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 8, 0, 1, 0, 0, 0, 6,
            1, 0, 2, 97, 103, 103, 108, 0, 112, 101, 114, 109, 105, 115, 115, 105, 118, 101, 0,
            112, 114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 14, 0, 1, 0, 0, 0, 6,
            0, 1, 1, 0, 0, 12, 1, 0, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112,
            114, 97, 101, 116, 0, 115, 112, 108, 105, 116, 0,
        ]
    );
    assert_eq!(parsed.separators, vec![32, 9]);
    assert_eq!(
        parsed.default_options,
        segment_rule_options("strict", "split")
    );
}

#[test]
fn parses_generator_segmentation_rules_config_like_legacy_pipeline() {
    let parsed = parse_segmentation_rules_from_str(
        "segmenty.dat",
        sample_segment_rules_config(),
        SegmentRulesTarget::Generator,
    )
    .unwrap();

    assert_eq!(
        parsed.segmentation_rules_data,
        vec![
            0, 0, 2, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97, 101,
            116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 8, 0, 1, 0, 0, 0, 6, 1, 0, 2, 97, 103,
            103, 108, 0, 112, 101, 114, 109, 105, 115, 115, 105, 118, 101, 0, 112, 114, 97, 101,
            116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 14, 0, 2, 0, 0, 0, 12, 1, 0, 0, 10, 1, 0,
            1, 0, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97, 101, 116,
            0, 115, 112, 108, 105, 116, 0,
        ]
    );
    assert!(parsed.separators.is_empty());
}

#[test]
fn applies_shift_orth_magic_like_legacy_pipeline() {
    let parsed = parse_segmentation_rules_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
        )
        .unwrap();

    assert_eq!(
        parsed.segmentation_rules_data,
        vec![
            0, 0, 1, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97, 101,
            116, 0, 115, 112, 108, 105, 116, 0, 0, 0, 0, 14, 0, 2, 0, 0, 0, 12, 1, 1, 0, 10, 1, 0,
            1, 0, 2, 97, 103, 103, 108, 0, 115, 116, 114, 105, 99, 116, 0, 112, 114, 97, 101, 116,
            0, 115, 112, 108, 105, 116, 0,
        ]
    );
    assert_eq!(
        parsed.shift_orth_extra_segment_types,
        BTreeMap::from([(0, 1)])
    );
    assert_eq!(parsed.replace_lemma_with_orth, BTreeSet::new());
    assert_eq!(
        parsed.additional_segment_type_names,
        BTreeMap::from([(1, "A>".to_owned())])
    );

    let only_shift = parse_segmentation_rules_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
        )
        .unwrap();
    assert_eq!(only_shift.shift_orth_extra_segment_types, BTreeMap::new());
    assert_eq!(only_shift.replace_lemma_with_orth, BTreeSet::from([0]));
}

#[test]
fn indexes_segment_types_like_legacy_segtypes_helper() {
    let parsed = parse_segmentation_rules_with_tagset_from_str(
        "segmenty.dat",
        sample_segment_type_resolver_config(),
        SegmentRulesTarget::Analyzer,
        &sample_segment_type_tagset(),
        &segment_type_names(),
        &segment_type_labels(),
    )
    .unwrap();
    let resolver = parsed.segment_type_resolver.as_ref().unwrap();

    for (base, tag_num, name_num, labels_num, expected_segment_type_num) in [
        ("kot", 10, 0, 0, 0),
        ("kot:1", 10, 0, 0, 1),
        ("kot:2", 10, 0, 0, 0),
        ("missing", 10, 0, 0, 4),
        ("missing", 11, 0, 0, 4),
        ("missing", 12, 0, 0, 5),
        ("named", 10, 1, 0, 2),
        ("named", 10, 2, 0, 4),
        ("labeled", 10, 0, 1, 3),
        ("labeled", 10, 0, 2, 3),
        ("labeled", 10, 0, 3, 4),
    ] {
        assert_eq!(
            resolver
                .lexeme_to_segment_type_num(base, tag_num, name_num, labels_num)
                .unwrap(),
            expected_segment_type_num,
            "{base}/{tag_num}/{name_num}/{labels_num}"
        );
        assert_eq!(
            parsed
                .lexeme_to_segment_type_num(base, tag_num, name_num, labels_num)
                .unwrap(),
            expected_segment_type_num,
            "parsed wrapper {base}/{tag_num}/{name_num}/{labels_num}"
        );
    }
}

#[test]
fn parsed_segment_rules_lookup_exposes_shift_orth_magic() {
    let parsed = parse_segmentation_rules_with_tagset_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
            &Tagset::from_str("tagset", "#!TAGSET-ID tid\n[TAGS]\n10\tsubst\n").unwrap(),
            &BTreeMap::from([(String::new(), 0)]),
            &BTreeMap::from([(QualifierSet::new(), 0)]),
        )
        .unwrap();

    assert_eq!(
        parsed
            .lexeme_to_segment_type_num("anything", 10, 0, 0)
            .unwrap(),
        0
    );
    assert_eq!(parsed.new_segment_type_for_shift_orth(0), Some(1));
    assert!(!parsed.should_replace_lemma_with_orth(0));

    let only_shift = parse_segmentation_rules_with_tagset_from_str(
            "segmenty.dat",
            "[options]\naggl = strict\npraet = split\n[combinations]\nA>\n[tags]\nA %\n[lexemes]\n[segment types]\nA\n[separator chars]\n",
            SegmentRulesTarget::Analyzer,
            &Tagset::from_str("tagset", "#!TAGSET-ID tid\n[TAGS]\n10\tsubst\n").unwrap(),
            &BTreeMap::from([(String::new(), 0)]),
            &BTreeMap::from([(QualifierSet::new(), 0)]),
        )
        .unwrap();
    assert!(only_shift.should_replace_lemma_with_orth(0));
    assert_eq!(only_shift.new_segment_type_for_shift_orth(0), None);
}

#[test]
fn rejects_too_many_qualifier_combinations() {
    let mut input = String::new();
    for index in 0..MAX_QUALIFIERS_COMBINATIONS {
        input.push_str(&format!("w{index}\tb{index}\ttag\t\tq{index}\n"));
    }

    let error = read_names_and_qualifiers_from_str("dict.tab", &input).unwrap_err();

    assert_eq!(
        error.to_string(),
        "Too many qualifiers combinations. The limit is 2048"
    );
}

#[test]
fn encodes_analyzer_forms_like_legacy_python_builder() {
    let cases = [
        (
            "kot",
            "kota",
            EncodedAnalyzerForm {
                prefix_cut_length: 0,
                cut_length: 0,
                suffix_to_add: "a".to_owned(),
                case_pattern: vec![false, false, false],
            },
        ),
        (
            "odkot",
            "kot",
            EncodedAnalyzerForm {
                prefix_cut_length: 2,
                cut_length: 0,
                suffix_to_add: String::new(),
                case_pattern: vec![false, false, false],
            },
        ),
        (
            "ABCd",
            "abcd",
            EncodedAnalyzerForm {
                prefix_cut_length: 0,
                cut_length: 0,
                suffix_to_add: String::new(),
                case_pattern: vec![false, false, false, false],
            },
        ),
        (
            "Lodz",
            "lodzi",
            EncodedAnalyzerForm {
                prefix_cut_length: 0,
                cut_length: 0,
                suffix_to_add: "i".to_owned(),
                case_pattern: vec![false, false, false, false],
            },
        ),
        (
            "abcdef",
            "zabcdef",
            EncodedAnalyzerForm {
                prefix_cut_length: 0,
                cut_length: 6,
                suffix_to_add: "zabcdef".to_owned(),
                case_pattern: vec![],
            },
        ),
        (
            "abcdef",
            "abcxyz",
            EncodedAnalyzerForm {
                prefix_cut_length: 0,
                cut_length: 3,
                suffix_to_add: "xyz".to_owned(),
                case_pattern: vec![false, false, false],
            },
        ),
    ];

    for (from, target, expected) in cases {
        assert_eq!(encode_analyzer_form(from, target).unwrap(), expected);
    }
}

#[test]
fn encodes_unicode_analyzer_form_like_legacy_python_builder() {
    assert_eq!(
        encode_analyzer_form("Łódź", "łodzi").unwrap(),
        EncodedAnalyzerForm {
            prefix_cut_length: 0,
            cut_length: 3,
            suffix_to_add: "odzi".to_owned(),
            case_pattern: vec![false],
        }
    );
}

#[test]
fn encodes_generator_forms_like_legacy_python_builder() {
    let cases = [
        (
            "kot",
            "kota",
            EncodedGeneratorForm {
                cut_length: 0,
                suffix_to_add: "a".to_owned(),
                prefix_to_add: String::new(),
            },
        ),
        (
            "odkot",
            "kot",
            EncodedGeneratorForm {
                cut_length: 4,
                suffix_to_add: "t".to_owned(),
                prefix_to_add: "k".to_owned(),
            },
        ),
        (
            "ABCd",
            "abcd",
            EncodedGeneratorForm {
                cut_length: 4,
                suffix_to_add: "abcd".to_owned(),
                prefix_to_add: String::new(),
            },
        ),
        (
            "abcdef",
            "zabcdef",
            EncodedGeneratorForm {
                cut_length: 0,
                suffix_to_add: String::new(),
                prefix_to_add: "z".to_owned(),
            },
        ),
        (
            "abcdef",
            "abcxyz",
            EncodedGeneratorForm {
                cut_length: 3,
                suffix_to_add: "xyz".to_owned(),
                prefix_to_add: String::new(),
            },
        ),
    ];

    for (from, target, expected) in cases {
        assert_eq!(encode_generator_form(from, target).unwrap(), expected);
    }
}

#[test]
fn encodes_unicode_generator_form_like_legacy_python_builder() {
    assert_eq!(
        encode_generator_form("Łódź", "łodzi").unwrap(),
        EncodedGeneratorForm {
            cut_length: 4,
            suffix_to_add: "łodzi".to_owned(),
            prefix_to_add: String::new(),
        }
    );
}

#[test]
fn analyzer_interpretation_sort_key_matches_legacy_python_builder() {
    let interpretation = AnalyzerInterpretation::new("Łódź", "łodzi", 7, 2, 3, 4).unwrap();

    assert_eq!(interpretation.orth_case_pattern, vec![true]);
    assert_eq!(interpretation.qualifiers, 4);
    assert_eq!(
        interpretation.sort_key(),
        AnalyzerInterpretationSortKey {
            cut_length: 3,
            prefix_cut_length: 0,
            suffix_to_add: vec!['o', 'd', 'z', 'i'],
            case_pattern: vec![false],
            orth_case_pattern: vec![true],
            tag_num: 7,
            name_num: 2,
            type_num: 3,
        }
    );
}

#[test]
fn generator_interpretation_sort_key_matches_legacy_python_builder() {
    let interpretation = GeneratorInterpretation::new("koty", "kot:1", 7, 2, 3, "1", 4).unwrap();

    assert_eq!(interpretation.lemma, "kot:1");
    assert_eq!(interpretation.qualifiers, 4);
    assert_eq!(
        interpretation.sort_key(),
        GeneratorInterpretationSortKey {
            homonym_id: "1".to_owned(),
            tag_num: 7,
            cut_length: 2,
            suffix_to_add: vec!['y'],
            name_num: 2,
            type_num: 3,
        }
    );
}

#[test]
fn rejects_empty_words_that_legacy_builder_asserts_on() {
    assert_eq!(
        encode_analyzer_form("", "lemma").unwrap_err().to_string(),
        "cannot encode analyzer form for an empty source word"
    );
    assert_eq!(
        encode_generator_form("lemma", "").unwrap_err().to_string(),
        "cannot encode generator form for an empty target word"
    );
}

#[test]
fn analyzer_converter_matches_legacy_grouping_shift_and_dedup_semantics() {
    let tagset = tagset();
    let names = names();
    let qualifiers_map = qualifiers_map();
    let rules = FakeRules::new()
        .with_type("kot", 10, 1, 1, 5)
        .with_type("kot", 10, 1, 2, 5)
        .with_type("base", 10, 1, 1, 6)
        .with_shift(6, 8)
        .with_type("lemma", 10, 0, 0, 9)
        .with_replace(9);

    let entries = convert_polimorf_for_analyzer(
        [
            "shift\tbase\ttag\tname\tq\n",
            "Kot\tkot\ttag\tname\tq\n",
            "kot\tkot\ttag\tname\tq\n",
            "kot\tkot\ttag\tname\tq2\n",
            "replace\tlemma\ttag\n",
        ],
        &tagset,
        &names,
        &qualifiers_map,
        &IdentityEncoder,
        &rules,
    )
    .unwrap();

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        vec!["kot", "replace", "shift"]
    );

    let kot = entry(&entries, "kot");
    assert_eq!(kot.interpretations.len(), 2);
    assert!(kot
        .interpretations
        .iter()
        .all(|interpretation| interpretation.qualifiers != 2));

    let replace = entry(&entries, "replace");
    assert_eq!(replace.interpretations.len(), 1);
    assert_eq!(replace.interpretations[0].type_num, 9);
    assert_eq!(replace.interpretations[0].encoded_form.cut_length, 0);
    assert_eq!(replace.interpretations[0].encoded_form.suffix_to_add, "");

    let shift_types = entry(&entries, "shift")
        .interpretations
        .iter()
        .map(|interpretation| interpretation.type_num)
        .collect::<BTreeSet<_>>();
    assert_eq!(shift_types, BTreeSet::from([6, 8]));
}

#[test]
fn analyzer_converter_rejects_replace_and_shift_conflict_like_legacy_assertion() {
    let tagset = tagset();
    let names = names();
    let qualifiers_map = qualifiers_map();
    let rules = FakeRules::new()
        .with_type("lemma", 10, 0, 0, 9)
        .with_replace(9)
        .with_shift(9, 8);

    let error = convert_polimorf_for_analyzer(
        ["orth\tlemma\ttag\n"],
        &tagset,
        &names,
        &qualifiers_map,
        &IdentityEncoder,
        &rules,
    )
    .unwrap_err();

    assert_eq!(
        error.to_string(),
        "shift-orth replacement and extra segment cannot both be active"
    );
}

#[test]
fn generator_converter_matches_legacy_homonym_shift_skip_and_dedup_semantics() {
    let tagset = tagset();
    let names = names();
    let qualifiers_map = qualifiers_map();
    let rules = FakeRules::new()
        .with_type("lemma", 10, 1, 1, 5)
        .with_type("lemma", 10, 1, 2, 5)
        .with_type("base", 10, 1, 1, 6)
        .with_shift(6, 8)
        .with_type("lemma", 10, 0, 0, 9)
        .with_replace(9);

    let entries = convert_polimorf_for_generator(
        [
            "form\tlemma:hid\ttag\tname\tq\n",
            "form\tlemma:hid\ttag\tname\tq\n",
            "form\tlemma:hid\ttag\tname\tq2\n",
            "empty\t\ttag\tname\tq\n",
            "shifted\tbase\ttag\tname\tq\n",
            "replace\tlemma\ttag\n",
        ],
        &tagset,
        &names,
        &qualifiers_map,
        &IdentityEncoder,
        &rules,
    )
    .unwrap();

    assert_eq!(
        entries
            .iter()
            .map(|entry| entry.key.as_str())
            .collect::<Vec<_>>(),
        vec!["base", "lemma", "replace", "shifted"]
    );

    let lemma = generator_entry(&entries, "lemma");
    assert_eq!(lemma.interpretations.len(), 1);
    assert_eq!(lemma.interpretations[0].homonym_id, "hid");
    assert_eq!(lemma.interpretations[0].qualifiers, 1);

    let base = generator_entry(&entries, "base");
    assert_eq!(base.interpretations[0].type_num, 6);

    let shifted = generator_entry(&entries, "shifted");
    assert_eq!(shifted.interpretations[0].type_num, 8);
    assert_eq!(shifted.interpretations[0].lemma, "shifted");

    let replace = generator_entry(&entries, "replace");
    assert_eq!(replace.interpretations[0].type_num, 9);
    assert_eq!(replace.interpretations[0].lemma, "replace");
}

#[test]
fn split_generator_homonym_matches_legacy_rules() {
    assert_eq!(
        split_generator_homonym("kot:1"),
        ("kot".to_owned(), "1".to_owned())
    );
    assert_eq!(
        split_generator_homonym(":1"),
        (":1".to_owned(), String::new())
    );
    assert_eq!(
        split_generator_homonym("kot:"),
        ("kot:".to_owned(), String::new())
    );
    assert_eq!(
        split_generator_homonym("kot:1:2"),
        ("kot".to_owned(), "1:2".to_owned())
    );
}

fn segment_types() -> BTreeMap<String, usize> {
    ["A", "B", "C", "D", "X", "Y", "Z", "SHIFT", "N"]
        .into_iter()
        .enumerate()
        .map(|(index, segment_type)| (segment_type.to_owned(), index + 1))
        .collect()
}

fn parsed_segment_rule(line: &str) -> SegmentRule {
    parsed_segment_rule_at(7, line)
}

fn parsed_segment_rule_at(line_number: usize, line: &str) -> SegmentRule {
    parse_segment_rule_line(line_number, line, &segment_types(), "<case>").unwrap()
}

fn parsed_segment_rules(lines: &[&str]) -> Vec<SegmentRule> {
    lines
        .iter()
        .enumerate()
        .map(|(index, line)| parsed_segment_rule_at(index + 7, line))
        .collect()
}

fn segment_rule_strings(rules: Vec<SegmentRule>) -> Vec<String> {
    rules.into_iter().map(|rule| rule.to_string()).collect()
}

fn segment_rule_options(aggl: &str, praet: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("aggl".to_owned(), aggl.to_owned()),
        ("praet".to_owned(), praet.to_owned()),
    ])
}

fn sample_segment_rules_config() -> &'static str {
    "[options]\n\
aggl = strict permissive\n\
praet = split\n\
\n\
[combinations]\n\
#ifdef strict\n\
A\n\
#endif\n\
#ifdef permissive\n\
A B\n\
#endif\n\
\n\
[tags]\n\
A %\n\
\n\
[lexemes]\n\
\n\
[segment types]\n\
A\n\
B\n\
\n\
[separator chars]\n\
32\n\
9\n"
}

fn sample_segment_type_resolver_config() -> &'static str {
    "[options]\n\
aggl = strict\n\
praet = split\n\
[combinations]\n\
LEX\n\
[tags]\n\
TAG_SUB subst:%\n\
TAG_ANY %\n\
[lexemes]\n\
LEX kot subst\n\
HOM kot:1 subst\n\
NAME named subst name=n1\n\
LABEL labeled subst labels=q\n\
[segment types]\n\
LEX\n\
HOM\n\
NAME\n\
LABEL\n\
TAG_SUB\n\
TAG_ANY\n\
[separator chars]\n"
}

fn sample_segment_type_tagset() -> Tagset {
    Tagset::from_str(
        "tagset",
        "#!TAGSET-ID tid\n[TAGS]\n10\tsubst\n11\tsubst:sg\n12\tadj\n",
    )
    .unwrap()
}

fn segment_type_names() -> BTreeMap<String, usize> {
    BTreeMap::from([
        (String::new(), 0),
        ("n1".to_owned(), 1),
        ("n2".to_owned(), 2),
    ])
}

fn segment_type_labels() -> BTreeMap<QualifierSet, usize> {
    BTreeMap::from([
        (qualifiers([]), 0),
        (qualifiers(["q"]), 1),
        (qualifiers(["q", "r"]), 2),
        (qualifiers(["r"]), 3),
    ])
}

fn qualifiers<const N: usize>(values: [&str; N]) -> QualifierSet {
    values.into_iter().map(str::to_owned).collect()
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn simple_oracle_state() -> SimpleState {
    SimpleState::accepting([0xaa, 0xbb])
        .with_transition(b'a', 0x010203, Some(9))
        .with_transition(b'b', 0x000102, Some(8))
        .with_transition(b'c', 0x000001, Some(7))
        .with_label_frequency(b'a', 5)
        .with_label_frequency(b'b', 5)
        .with_label_frequency(b'c', 1)
}

fn simple_global_frequencies() -> BTreeMap<u8, usize> {
    BTreeMap::from([(b'a', 3), (b'b', 7), (b'c', 1)])
}

fn simple_dictionary_metadata() -> (
    Tagset,
    BTreeMap<String, usize>,
    BTreeMap<QualifierSet, usize>,
) {
    (
        Tagset::from_str("sample.tagset", "#!TAGSET-ID tid\n[TAGS]\n2\tb\n1\ta\n").unwrap(),
        BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)]),
        BTreeMap::from([
            (qualifiers([]), 0),
            (qualifiers(["x"]), 1),
            (qualifiers(["arch", "rare"]), 2),
        ]),
    )
}

fn constructed_simple_entries() -> Vec<(Vec<u8>, Vec<u8>)> {
    vec![
        (b"a".to_vec(), vec![1]),
        (b"ab".to_vec(), vec![2]),
        (b"b".to_vec(), vec![3]),
        (b"bb".to_vec(), vec![2]),
    ]
}

fn simple_oracle_graph(with_transition_data: bool) -> SimpleFsaGraph {
    let transition_data = |value| {
        if with_transition_data {
            Some(value)
        } else {
            None
        }
    };
    SimpleFsaGraph {
        states: vec![
            SimpleGraphState::non_accepting()
                .with_frequency(0)
                .with_transition(b'a', 1, transition_data(9))
                .with_transition(b'b', 2, transition_data(8))
                .with_label_frequency(b'a', 10)
                .with_label_frequency(b'b', 1),
            SimpleGraphState::non_accepting()
                .with_frequency(1)
                .with_transition(b'x', 3, transition_data(3))
                .with_label_frequency(b'x', 1),
            SimpleGraphState::non_accepting()
                .with_frequency(5)
                .with_transition(b'c', 3, transition_data(4))
                .with_label_frequency(b'c', 1),
            SimpleGraphState::accepting([0xde, 0xad]).with_frequency(0),
        ],
        initial_state: 0,
        global_label_frequencies: BTreeMap::from([(b'a', 5), (b'b', 2), (b'c', 7), (b'x', 1)]),
    }
}

fn tagset() -> BTreeMap<String, usize> {
    BTreeMap::from([("tag".to_owned(), 10)])
}

fn names() -> BTreeMap<String, usize> {
    BTreeMap::from([(String::new(), 0), ("name".to_owned(), 1)])
}

fn qualifiers_map() -> BTreeMap<QualifierSet, usize> {
    BTreeMap::from([
        (qualifiers([]), 0),
        (qualifiers(["q"]), 1),
        (qualifiers(["q2"]), 2),
    ])
}

fn entry<'a>(entries: &'a [AnalyzerEntry], key: &str) -> &'a AnalyzerEntry {
    entries
        .iter()
        .find(|entry| entry.key == key)
        .unwrap_or_else(|| panic!("missing analyzer entry {key}"))
}

fn generator_entry<'a>(entries: &'a [GeneratorEntry], key: &str) -> &'a GeneratorEntry {
    entries
        .iter()
        .find(|entry| entry.key == key)
        .unwrap_or_else(|| panic!("missing generator entry {key}"))
}

#[derive(Debug, Clone, Default)]
struct FakeRules {
    types: BTreeMap<(String, usize, usize, usize), usize>,
    replacements: BTreeSet<usize>,
    shifts: BTreeMap<usize, usize>,
}

impl FakeRules {
    fn new() -> Self {
        Self::default()
    }

    fn with_type(
        mut self,
        base: &str,
        tag_num: usize,
        name_num: usize,
        qualifiers_num: usize,
        segment_type_num: usize,
    ) -> Self {
        self.types.insert(
            (base.to_owned(), tag_num, name_num, qualifiers_num),
            segment_type_num,
        );
        self
    }

    fn with_replace(mut self, segment_type_num: usize) -> Self {
        self.replacements.insert(segment_type_num);
        self
    }

    fn with_shift(mut self, segment_type_num: usize, new_segment_type_num: usize) -> Self {
        self.shifts.insert(segment_type_num, new_segment_type_num);
        self
    }
}

impl SegmentRulesLookup for FakeRules {
    fn lexeme_to_segment_type_num(
        &self,
        base: &str,
        tag_num: usize,
        name_num: usize,
        qualifiers_num: usize,
    ) -> Result<usize> {
        self.types
            .get(&(base.to_owned(), tag_num, name_num, qualifiers_num))
            .copied()
            .ok_or_else(|| {
                BuilderError::new(format!(
                    "missing fake segment type for {base}/{tag_num}/{name_num}/{qualifiers_num}"
                ))
            })
    }

    fn should_replace_lemma_with_orth(&self, segment_type_num: usize) -> bool {
        self.replacements.contains(&segment_type_num)
    }

    fn new_segment_type_for_shift_orth(&self, segment_type_num: usize) -> Option<usize> {
        self.shifts.get(&segment_type_num).copied()
    }
}
