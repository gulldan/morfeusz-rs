from pathlib import Path

import pytest

import morfeusz2_rs as morfeusz2  # drop-in: same API under the renamed module


REAL_SGJP = Path("/tmp/morfeusz-sgjp-20260601")


def require_real_sgjp():
    analyzer = REAL_SGJP / "sgjp-a.dict"
    generator = REAL_SGJP / "sgjp-s.dict"
    if not analyzer.exists() or not generator.exists():
        pytest.skip(f"real SGJP dictionary is missing under {REAL_SGJP}")


def test_python_binding_analyzes_and_generates_with_real_sgjp():
    require_real_sgjp()
    old_paths = morfeusz2._Morfeusz_dictionarySearchPaths_get()
    try:
        morfeusz2._Morfeusz_dictionarySearchPaths_set((str(REAL_SGJP),))

        low = morfeusz2._Morfeusz.createInstance("sgjp")
        analyzed = low.analyse("zażółć")
        assert len(analyzed) == 1
        assert analyzed[0].orth == "zażółć"
        assert analyzed[0].lemma == "zażółcić"
        assert analyzed[0].getTag(low) == "impt:sg:sec:perf"

        tag_id = low.getIdResolver().getTagId("impt:sg:sec:perf")
        generated = low.generate("zażółcić", tag_id)
        assert any(item.orth == "zażółć" and item.lemma == "zażółcić" for item in generated)

        high = morfeusz2.Morfeusz(dict_name="sgjp", dict_path=str(REAL_SGJP))
        assert (0, 1, ("zażółć", "zażółcić", "impt:sg:sec:perf", [], [])) in high.analyse(
            "zażółć"
        )
        assert ("zażółć", "zażółcić", "impt:sg:sec:perf", [], []) in high.generate(
            "zażółcić", tag_id
        )
    finally:
        morfeusz2._Morfeusz_dictionarySearchPaths_set(old_paths)
