import morfeusz2
import pytest
import shutil
from importlib import metadata
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BINARY_FIXTURES = ROOT / "morfeusz-rs" / "tests" / "fixtures" / "binary"


@pytest.fixture(autouse=True)
def default_dictionary_search_path(tmp_path):
    shutil.copy(BINARY_FIXTURES / "test-dict-copyright-v1-a.dict", tmp_path / "sgjp-a.dict")
    shutil.copy(BINARY_FIXTURES / "test-digits-v1-s.dict", tmp_path / "sgjp-s.dict")
    old_paths = morfeusz2._Morfeusz_dictionarySearchPaths_get()
    morfeusz2._Morfeusz_dictionarySearchPaths_set((str(tmp_path),))
    try:
        yield tmp_path
    finally:
        morfeusz2._Morfeusz_dictionarySearchPaths_set(old_paths)


def test_module_metadata_matches_python_wrapper_surface():
    package_metadata = metadata.metadata("morfeusz2")

    assert morfeusz2.__version__ == morfeusz2.Morfeusz.getVersion()
    assert morfeusz2.__version__ == "1.99.15"
    assert metadata.version("morfeusz2") == morfeusz2.__version__
    assert package_metadata["Name"] == "morfeusz2"
    assert package_metadata["Version"] == morfeusz2.__version__
    assert package_metadata["Summary"] == "Python bindings for Morfeusz 2"
    assert package_metadata["Requires-Python"] == ">=3.9"
    assert "Programming Language :: Rust" in package_metadata.get_all("Classifier")
    assert "Homepage, https://morfeusz.sgjp.pl" in package_metadata.get_all(
        "Project-URL"
    )
    assert "Repository, https://github.com/sgjp/morfeusz" in package_metadata.get_all(
        "Project-URL"
    )
    assert morfeusz2._Morfeusz is not morfeusz2.Morfeusz
    assert morfeusz2._Morfeusz.getVersion() == morfeusz2.Morfeusz.getVersion()
    assert morfeusz2._Morfeusz_getVersion() == morfeusz2.Morfeusz.getVersion()
    assert (
        morfeusz2._Morfeusz_getDefaultDictName()
        == morfeusz2.Morfeusz.getDefaultDictName()
    )
    assert morfeusz2.Morfeusz.getDefaultDictName() == "sgjp"
    assert morfeusz2._Morfeusz_getCopyright() == morfeusz2.Morfeusz.getCopyright()
    assert morfeusz2.__copyright__ == morfeusz2.Morfeusz.getCopyright()
    assert "Copyright © 2014–2021" in morfeusz2.__copyright__
    assert morfeusz2.GENDERS == ["m1", "m2", "m3", "f", "n"]


def test_legacy_container_aliases_are_exposed():
    interp = morfeusz2.MorphInterpretation.createIgn(0, 1, "x", "x")

    interps = morfeusz2.InterpsList([interp])
    strings = morfeusz2.StringsList(["a", "b"])
    linked = morfeusz2.StringsLinkedList(["a", "b"])
    labels = morfeusz2.StringsSet(["b", "a", "a"])

    assert interps[0].isIgn()
    assert strings == ["a", "b"]
    assert linked == ["a", "b"]
    assert labels == {"a", "b"}


def test_enum_constants_match_cpp_header_values():
    assert morfeusz2.UTF8 == 11
    assert morfeusz2.ISO8859_2 == 12
    assert morfeusz2.CP1250 == 13
    assert morfeusz2.CP852 == 14

    assert morfeusz2.CONDITIONALLY_CASE_SENSITIVE == 100
    assert morfeusz2.STRICTLY_CASE_SENSITIVE == 101
    assert morfeusz2.IGNORE_CASE == 102

    assert morfeusz2.SEPARATE_NUMBERING == 201
    assert morfeusz2.CONTINUOUS_NUMBERING == 202

    assert morfeusz2.SKIP_WHITESPACES == 301
    assert morfeusz2.APPEND_WHITESPACES == 302
    assert morfeusz2.KEEP_WHITESPACES == 303

    assert morfeusz2.ANALYSE_ONLY == 401
    assert morfeusz2.GENERATE_ONLY == 402
    assert morfeusz2.BOTH_ANALYSE_AND_GENERATE == 403


def test_setters_and_getters_round_trip_wrapper_enums():
    morf = morfeusz2._Morfeusz._createInstance()

    morf.setCharset(morfeusz2.CP1250)
    assert morf.getCharset() == morfeusz2.CP1250
    morf.setCharset(morfeusz2.UTF8)
    assert morf.getCharset() == morfeusz2.UTF8

    morf.setCaseHandling(morfeusz2.STRICTLY_CASE_SENSITIVE)
    assert morf.getCaseHandling() == morfeusz2.STRICTLY_CASE_SENSITIVE
    morf.setCaseHandling(morfeusz2.IGNORE_CASE)
    assert morf.getCaseHandling() == morfeusz2.IGNORE_CASE

    morf.setTokenNumbering(morfeusz2.CONTINUOUS_NUMBERING)
    assert morf.getTokenNumbering() == morfeusz2.CONTINUOUS_NUMBERING
    morf.setTokenNumbering(morfeusz2.SEPARATE_NUMBERING)
    assert morf.getTokenNumbering() == morfeusz2.SEPARATE_NUMBERING

    morf.setWhitespaceHandling(morfeusz2.APPEND_WHITESPACES)
    assert morf.getWhitespaceHandling() == morfeusz2.APPEND_WHITESPACES
    morf.setWhitespaceHandling(morfeusz2.KEEP_WHITESPACES)
    assert morf.getWhitespaceHandling() == morfeusz2.KEEP_WHITESPACES

    assert morf.getAggl() in morf.getAvailableAgglOptions()
    assert morf.getPraet() in morf.getAvailablePraetOptions()


def test_low_level_analyse_returns_morph_interpretation_list():
    morf = morfeusz2._Morfeusz._createInstance()
    result = morf.analyse("Aaaa żżżż")

    assert len(result) == 2
    assert result[0].orth == "Aaaa"
    assert result[1].orth == "żżżż"


def test_high_level_analyse_returns_shadow_tuples_by_default():
    morf = morfeusz2.Morfeusz()
    result = morf.analyse("Aaaa żżżż")

    assert result == [
        (0, 1, ("Aaaa", "Aaaa", "ign", [], [])),
        (1, 2, ("żżżż", "żżżż", "ign", [], [])),
    ]


def test_default_constructor_loads_default_dictionary_name():
    morf = morfeusz2.Morfeusz()

    assert morf.dict_id() == "identyfikator_słownika"
    assert (0, 1, ("7", "7", "dig", [], [])) in morf.analyse("7")
    assert ("123", "123", "dig", [], []) in morf.generate("123")


def test_low_level_create_instance_loads_default_dictionary_name():
    morf = morfeusz2._Morfeusz._createInstance(morfeusz2.ANALYSE_ONLY)

    assert any(item.orth == "7" and item.getTag(morf) == "dig" for item in morf.analyse("7"))


def test_create_instance_accepts_legacy_positional_usage():
    low = morfeusz2._Morfeusz.createInstance(morfeusz2.ANALYSE_ONLY)
    high = morfeusz2.Morfeusz.createInstance(morfeusz2.ANALYSE_ONLY)

    assert any(item.orth == "7" and item.getTag(low) == "dig" for item in low.analyse("7"))
    assert (0, 1, ("7", "7", "dig", [], [])) in high.analyse("7")

    with pytest.raises(RuntimeError):
        low.generate("123")
    with pytest.raises(RuntimeError):
        high.generate("123")


def test_keep_whitespace():
    morf = morfeusz2._Morfeusz._createInstance()
    morf.setWhitespaceHandling(morfeusz2.KEEP_WHITESPACES)
    result = morf.analyse("Aaaa  żżżż")

    assert [item.orth for item in result] == ["Aaaa", "  ", "żżżż"]
    assert result[1].isWhitespace()


def test_analyse_iter_matches_wrapper_iterator_shape():
    morf = morfeusz2._Morfeusz._createInstance()

    iterator = morf.analyse_iter("Aaaa żżżż")
    assert iterator.hasNext()
    assert iterator.peek().orth == "Aaaa"
    assert iterator.next().orth == "Aaaa"
    assert iterator.hasNext()
    assert iterator.next().orth == "żżżż"
    assert not iterator.hasNext()
    with pytest.raises(RuntimeError):
        iterator.peek()
    with pytest.raises(StopIteration):
        iterator.next()

    assert [item.orth for item in morf.analyse_iter("Aaaa żżżż")] == [
        "Aaaa",
        "żżżż",
    ]


def test_low_level_swig_aliases_match_python_wrapper_shape():
    morf = morfeusz2._Morfeusz._createInstance(morfeusz2.ANALYSE_ONLY)

    iterator = morf._analyseAsIterator("Aaaa")

    assert iterator.hasNext()
    assert iterator.next().orth == "Aaaa"
    assert not iterator.hasNext()


def test_morph_interpretation_shadow_storage_aliases():
    interp = morfeusz2.MorphInterpretation.createIgn(0, 1, "orth", "lemma")

    assert interp._orth == "orth"
    assert interp._lemma == "lemma"
    interp._orth = "changed orth"
    interp._lemma = "changed lemma"

    assert interp.orth == "changed orth"
    assert interp.lemma == "changed lemma"


def test_invalid_options_raise_value_error():
    morf = morfeusz2._Morfeusz._createInstance()

    for call in (
        lambda: morf.setCharset(0),
        lambda: morf.setCaseHandling(0),
        lambda: morf.setTokenNumbering(0),
        lambda: morf.setWhitespaceHandling(0),
        lambda: morfeusz2.Morfeusz(usage=0),
    ):
        try:
            call()
        except ValueError:
            pass
        else:
            raise AssertionError("invalid enum value should raise ValueError")


def test_invalid_domain_operations_raise_runtime_error():
    morf = morfeusz2._Morfeusz._createInstance()
    high_level = morfeusz2.Morfeusz()
    invalid_tag_id = morf.getIdResolver().getTagsCount()

    for call in (
        lambda: morf.setAggl("XXXXYYYYZZZZ"),
        lambda: morf.setPraet("XXXXYYYYZZZZ"),
        lambda: morf.generate("AAAA BBBB"),
        lambda: morf.generate("123", invalid_tag_id),
        lambda: morf._generateByTagId("123", invalid_tag_id),
        lambda: high_level.generate("123", invalid_tag_id),
    ):
        try:
            call()
        except RuntimeError:
            pass
        else:
            raise AssertionError("invalid domain operation should raise RuntimeError")


def test_invalid_id_lookups_raise_runtime_error_like_swig():
    morf = morfeusz2._Morfeusz._createInstance()
    high_level = morfeusz2.Morfeusz.createInstance()
    resolver = morf.getIdResolver()
    interp = morfeusz2.MorphInterpretation()
    interp.tagId = resolver.getTagsCount()
    interp.nameId = resolver.getNamesCount()
    interp.labelsId = resolver.getLabelsCount()

    for call in (
        lambda: resolver.getTag(resolver.getTagsCount()),
        lambda: resolver.getName(resolver.getNamesCount()),
        lambda: resolver.getLabelsAsString(resolver.getLabelsCount()),
        lambda: resolver.getLabelsAsUnicode(resolver.getLabelsCount()),
        lambda: resolver.getLabels(resolver.getLabelsCount()),
        lambda: interp.getTag(morf),
        lambda: interp.getName(morf),
        lambda: interp.getLabelsAsString(morf),
        lambda: interp.getLabelsAsUnicode(morf),
        lambda: interp.getLabels(morf),
        lambda: high_level._interp2tuple(interp),
    ):
        with pytest.raises(RuntimeError):
            call()


def test_non_existing_dictionary_raises_io_error():
    morf = morfeusz2._Morfeusz._createInstance()

    try:
        morf.setDictionary("definitely_missing_dictionary")
    except OSError:
        pass
    else:
        raise AssertionError("missing dictionary should raise OSError")


def test_create_instance_missing_dictionary_raises_runtime_error():
    for factory in (
        lambda: morfeusz2._Morfeusz.createInstance("definitely_missing_dictionary"),
        lambda: morfeusz2.Morfeusz.createInstance("definitely_missing_dictionary"),
        lambda: morfeusz2.Morfeusz(dict_name="definitely_missing_dictionary"),
    ):
        with pytest.raises(RuntimeError):
            factory()


def test_invalid_dictionary_file_raises_io_error(tmp_path):
    (tmp_path / "broken-a.dict").write_text("IzEne9FXuc", encoding="utf-8")
    morf = morfeusz2._Morfeusz._createInstance()
    old_paths = morfeusz2._Morfeusz_dictionarySearchPaths_get()
    try:
        morfeusz2._Morfeusz_dictionarySearchPaths_set((str(tmp_path),))

        try:
            morf.setDictionary("broken")
        except OSError:
            pass
        else:
            raise AssertionError("invalid dictionary should raise OSError")
    finally:
        morfeusz2._Morfeusz_dictionarySearchPaths_set(old_paths)


def test_create_instance_invalid_dictionary_file_raises_io_error(tmp_path):
    (tmp_path / "broken-a.dict").write_text("IzEne9FXuc", encoding="utf-8")
    old_paths = morfeusz2._Morfeusz_dictionarySearchPaths_get()
    try:
        morfeusz2._Morfeusz_dictionarySearchPaths_set((str(tmp_path),))

        with pytest.raises(OSError):
            morfeusz2._Morfeusz.createInstance("broken", morfeusz2.ANALYSE_ONLY)
    finally:
        morfeusz2._Morfeusz_dictionarySearchPaths_set(old_paths)


def test_loads_binary_analyzer_dictionary_path():
    dictionary = BINARY_FIXTURES / "test-dict-copyright-v1-a.dict"
    morf = morfeusz2._Morfeusz.createInstance(str(dictionary))

    result = morf.analyse("7")

    assert any(item.orth == "7" and item.getTag(morf) == "dig" for item in result)


def test_loads_binary_generator_dictionary_path():
    dictionary = BINARY_FIXTURES / "test-digits-v1-s.dict"
    morf = morfeusz2._Morfeusz.createInstance(str(dictionary))

    result = morf.generate("123")

    assert any(item.orth == "123" and item.getTag(morf) == "dig" for item in result)

    tag_id = morf.getIdResolver().getTagId("dig")
    low_level = morf._generateByTagId("123", tag_id)
    assert any(item.orth == "123" and item.getTag(morf) == "dig" for item in low_level)


def test_named_binary_dictionary_lookup(tmp_path, monkeypatch):
    shutil.copy(BINARY_FIXTURES / "test-dict-copyright-v1-a.dict", tmp_path / "named-a.dict")
    shutil.copy(BINARY_FIXTURES / "test-digits-v1-s.dict", tmp_path / "named-s.dict")
    monkeypatch.chdir(tmp_path)

    morf = morfeusz2._Morfeusz.createInstance("named")
    assert any(item.orth == "7" and item.getTag(morf) == "dig" for item in morf.analyse("7"))
    assert any(item.orth == "123" and item.getTag(morf) == "dig" for item in morf.generate("123"))

    other = morfeusz2._Morfeusz._createInstance()
    other.setDictionary("named")
    assert any(item.orth == "7" and item.getTag(other) == "dig" for item in other.analyse("7"))


def test_set_dictionary_preserves_runtime_options_and_resets_segmentation(tmp_path):
    shutil.copy(BINARY_FIXTURES / "test-dict-copyright-v1-a.dict", tmp_path / "switch-a.dict")
    shutil.copy(BINARY_FIXTURES / "test-digits-v1-s.dict", tmp_path / "switch-s.dict")
    old_paths = morfeusz2._Morfeusz_dictionarySearchPaths_get()
    try:
        morfeusz2._Morfeusz_dictionarySearchPaths_set((str(tmp_path),))
        fresh_default = morfeusz2._Morfeusz.createInstance("switch").getAggl()
        explicit_aggl = next(
            option
            for option in ("strict", "permissive", "isolated")
            if option != fresh_default
        )
        morf = morfeusz2._Morfeusz._createInstance()
        morf.setWhitespaceHandling(morfeusz2.KEEP_WHITESPACES)
        morf.setCaseHandling(morfeusz2.IGNORE_CASE)
        morf.setTokenNumbering(morfeusz2.CONTINUOUS_NUMBERING)
        morf.setAggl(explicit_aggl)

        morf.setDictionary("switch")

        assert morf.getWhitespaceHandling() == morfeusz2.KEEP_WHITESPACES
        assert morf.getCaseHandling() == morfeusz2.IGNORE_CASE
        assert morf.getTokenNumbering() == morfeusz2.CONTINUOUS_NUMBERING
        assert morf.getAggl() == fresh_default
        assert any(item.isWhitespace() for item in morf.analyse("7 7"))
    finally:
        morfeusz2._Morfeusz_dictionarySearchPaths_set(old_paths)


def test_constructor_accepts_shadow_dict_name_and_dict_path(tmp_path):
    shutil.copy(BINARY_FIXTURES / "test-dict-copyright-v1-a.dict", tmp_path / "shadow-a.dict")
    shutil.copy(BINARY_FIXTURES / "test-digits-v1-s.dict", tmp_path / "shadow-s.dict")

    morf = morfeusz2.Morfeusz(dict_name="shadow", dict_path=str(tmp_path))

    assert (0, 1, ("7", "7", "dig", [], [])) in morf.analyse("7")
    assert ("123", "123", "dig", [], []) in morf.generate("123")


def test_dictionary_search_paths_global_get_set_and_shadow_aliases(tmp_path):
    shutil.copy(BINARY_FIXTURES / "test-dict-copyright-v1-a.dict", tmp_path / "paths-a.dict")
    shutil.copy(BINARY_FIXTURES / "test-digits-v1-s.dict", tmp_path / "paths-s.dict")
    old_paths = morfeusz2._Morfeusz_dictionarySearchPaths_get()
    try:
        morfeusz2._Morfeusz_dictionarySearchPaths_set((str(tmp_path),))

        assert morfeusz2._Morfeusz_dictionarySearchPaths_get() == [str(tmp_path)]
        morf = morfeusz2._Morfeusz.createInstance("paths")
        assert any(item.orth == "7" and item.getTag(morf) == "dig" for item in morf.analyse("7"))
        assert morf.dict_id() == morf.getDictID()
        assert morf.dict_copyright() == morf.getDictCopyright()

        other_dir = tmp_path / "other"
        other_dir.mkdir()
        morf.add_dictionary_path(str(other_dir))
        assert morfeusz2._Morfeusz_dictionarySearchPaths_get()[0] == str(other_dir)
    finally:
        morfeusz2._Morfeusz_dictionarySearchPaths_set(old_paths)


def test_shadow_expand_tags_splits_dot_variants():
    dictionary = BINARY_FIXTURES / "test-inflection-graph-numbers-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary), expand_tags=True)

    result = morf.analyse("rad")

    assert (0, 1, ("rad", "rad:v", "winien:sg:m1:imperf", [], [])) in result
    assert (0, 1, ("rad", "rad:v", "winien:sg:m2:imperf", [], [])) in result
    assert (0, 1, ("rad", "rad:v", "winien:sg:m3:imperf", [], [])) in result


def test_shadow_expand_tags_can_preserve_dot_groups():
    dictionary = BINARY_FIXTURES / "test-inflection-graph-numbers-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary), expand_tags=True, expand_dot=False)

    result = morf.analyse("rad")

    assert result == [
        (0, 1, ("rad", "rad:v", "winien:sg:m1.m2.m3:imperf", [], []))
    ]


def test_shadow_expand_dag_returns_paths_without_node_numbers():
    dictionary = BINARY_FIXTURES / "test-inflection-graph-numbers-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary), expand_dag=True)

    paths = morf.analyse("radem,")

    assert [
        ("rad", "rad:v", "winien:sg:m1.m2.m3:imperf", [], []),
        ("em", "być", "aglt:sg:pri:imperf:wok", [], []),
        (",", ",", "interp", [], []),
    ] in paths
    assert [
        ("radem", "rad:s", "subst:sg:inst:m3", ["nazwa pospolita"], []),
        (",", ",", "interp", [], []),
    ] in paths


def test_shadow_helper_methods_match_swig_surface():
    dictionary = BINARY_FIXTURES / "test-dict-copyright-v1-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary))

    assert list(morf._expand_tag("a.b:_")) == [
        "a:m1",
        "a:m2",
        "a:m3",
        "a:f",
        "a:n",
        "b:m1",
        "b:m2",
        "b:m3",
        "b:f",
        "b:n",
    ]

    assert list(morf._expand_interp(("orth", "lemma", "a.b", [], []))) == [
        ("orth", "lemma", "a", [], []),
        ("orth", "lemma", "b", [], []),
    ]

    assert morfeusz2.Morfeusz._dag_to_list(
        [
            (0, 1, ("a", "a", "ign", [], [])),
            (1, 2, ("b", "b", "ign", [], [])),
            (0, 2, ("ab", "ab", "ign", [], [])),
        ]
    ) == [
        [("a", "a", "ign", [], []), ("b", "b", "ign", [], [])],
        [("ab", "ab", "ign", [], [])],
    ]

    low = morfeusz2._Morfeusz.createInstance(str(dictionary))
    digit = next(item for item in low.analyse("7") if item.getTag(low) == "dig")
    assert morf._interp2tuple(digit) == ("7", "7", "dig", [], [])


def test_shadow_expand_flags_are_public_mutable_attributes():
    dictionary = BINARY_FIXTURES / "test-inflection-graph-numbers-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary))

    assert morf.expand_dag is False
    assert morf.expand_tags is False
    assert morf.expand_dot is True
    assert morf.expand_underscore is True

    morf.expand_tags = True
    assert (0, 1, ("rad", "rad:v", "winien:sg:m1:imperf", [], [])) in morf.analyse("rad")

    morf.expand_dot = False
    assert morf._expand_tag("a.b:_") == ["a.b:m1.m2.m3.f.n"]

    morf.expand_underscore = False
    assert morf._expand_tag("a.b:_") == ["a.b:_"]

    morf.expand_tags = False
    morf.expand_dag = True
    assert [
        ("rad", "rad:v", "winien:sg:m1.m2.m3:imperf", [], []),
        ("em", "być", "aglt:sg:pri:imperf:wok", [], []),
        (",", ",", "interp", [], []),
    ] in morf.analyse("radem,")


def test_shadow_wrapper_has_python_instance_dict_and_clone_keeps_flags():
    dictionary = BINARY_FIXTURES / "test-inflection-graph-numbers-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary), expand_tags=True, expand_dot=False)
    morf.user_state = {"caller": "kept"}

    assert morf.__dict__["user_state"] == {"caller": "kept"}
    assert isinstance(morf._morfeusz_obj, morfeusz2._Morfeusz)
    assert morf._expand_tag("a.b:_") == ["a.b:m1.m2.m3.f.n"]

    cloned = morf.clone()
    assert cloned.expand_tags is True
    assert cloned.expand_dot is False
    assert cloned._expand_tag("a.b:_") == ["a.b:m1.m2.m3.f.n"]
    assert isinstance(cloned._morfeusz_obj, morfeusz2._Morfeusz)


def test_shadow_wrapper_low_level_object_is_live():
    dictionary = BINARY_FIXTURES / "test-dict-copyright-v1-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary))

    morf._morfeusz_obj.setWhitespaceHandling(morfeusz2.KEEP_WHITESPACES)

    assert morf.getWhitespaceHandling() == morfeusz2.KEEP_WHITESPACES
    assert (1, 2, ("  ", "  ", "sp", [], [])) in morf.analyse("7  7")


def test_shadow_wrapper_low_level_object_assignment_is_live():
    dictionary = BINARY_FIXTURES / "test-dict-copyright-v1-a.dict"
    morf = morfeusz2.Morfeusz(str(dictionary))
    replacement = morfeusz2._Morfeusz.createInstance(str(dictionary))
    replacement.setWhitespaceHandling(morfeusz2.KEEP_WHITESPACES)

    morf._morfeusz_obj = replacement

    assert morf.getWhitespaceHandling() == morfeusz2.KEEP_WHITESPACES
    assert (1, 2, ("  ", "  ", "sp", [], [])) in morf.analyse("7  7")


def test_constructor_accepts_shadow_usage_flags(tmp_path):
    shutil.copy(BINARY_FIXTURES / "test-digits-v1-s.dict", tmp_path / "genonly-s.dict")

    morf = morfeusz2.Morfeusz(
        dict_name="genonly",
        dict_path=str(tmp_path),
        analyse=False,
        generate=True,
    )

    assert ("123", "123", "dig", [], []) in morf.generate("123")

    try:
        morf.analyse("123")
    except RuntimeError:
        pass
    else:
        raise AssertionError("generate-only constructor should reject analyse")


def test_morph_interpretation_name_matches_python_wrapper_shape():
    dictionary = BINARY_FIXTURES / "test-names-a.dict"
    morf = morfeusz2._Morfeusz.createInstance(str(dictionary))

    result = morf.analyse("czerwony")
    named = next(item for item in result if item.lemma == "czerwony:a3")

    assert named.getName(morf) == ["zażółć gęślą jaźń"]


def test_morph_interpretation_labels_match_python_wrapper_shape():
    dictionary = BINARY_FIXTURES / "test-qualifiers-a.dict"
    morf = morfeusz2._Morfeusz.createInstance(str(dictionary))

    result = morf.analyse("czerwony")
    labelled = next(item for item in result if item.lemma == "czerwony:a4")

    assert labelled.getLabels(morf) == ["żółty1", "żółty2", "żółty3"]
    assert labelled.getLabelsAsUnicode(morf) == "żółty1|żółty2|żółty3"
    assert labelled.getLabels(morf) == sorted(labelled.getLabelsAsUnicode(morf).split("|"))


def test_id_resolver_labels_match_python_wrapper_shape():
    dictionary = BINARY_FIXTURES / "test-qualifiers-a.dict"
    morf = morfeusz2._Morfeusz.createInstance(str(dictionary))
    resolver = morf.getIdResolver()

    labels_id = resolver.getLabelsId("żółty1|żółty2|żółty3")

    assert resolver.getLabels(labels_id) == ["żółty1", "żółty2", "żółty3"]
    assert resolver.getLabelsAsUnicode(labels_id) == "żółty1|żółty2|żółty3"
    assert resolver.getLabels(labels_id) == sorted(
        resolver.getLabelsAsUnicode(labels_id).split("|")
    )


def test_morph_interpretation_static_constructors():
    ign = morfeusz2.MorphInterpretation.createIgn(3, 4, "orth", "lemma")
    whitespace = morfeusz2.MorphInterpretation.createWhitespace(5, 6, "  ")

    assert ign.startNode == 3
    assert ign.endNode == 4
    assert ign.orth == "orth"
    assert ign.lemma == "lemma"
    assert ign.isIgn()

    assert whitespace.startNode == 5
    assert whitespace.endNode == 6
    assert whitespace.orth == "  "
    assert whitespace.lemma == "  "
    assert whitespace.isWhitespace()
