# Binary Fixtures

`test-dict-copyright-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_dict_copyright` with `--only-analyzer` and
`--serialization-method V2`.

`test-dict-copyright-v1-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_dict_copyright` with `--only-analyzer` and
`--serialization-method V1`.

`test-dict-copyright-simple-a.dict` is generated from the C++/Python builder
using `tests/analyzer/test_dict_copyright` with `--only-analyzer` and
`--serialization-method SIMPLE`.

`test-digits-roman-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_digits_roman` with `--only-analyzer` and
`--serialization-method V2`.

`test-digits-s.dict` is generated from the C++/Python builder using
`tests/generator/test_digits` with `--only-generator` and
`--serialization-method V2`.

`test-digits-v1-s.dict` is generated from the C++/Python builder using
`tests/generator/test_digits` with `--only-generator` and
`--serialization-method V1`.

`test-digits-simple-s.dict` is generated from the C++/Python builder using
`tests/generator/test_digits` with `--only-generator` and
`--serialization-method SIMPLE`.

`test-inflection-graph-numbers-a.dict` is generated from the C++/Python builder
using `tests/analyzer/test_inflection_graph_numbers` with `--only-analyzer` and
`--serialization-method V2`.

`test-additional-atomic-s.dict` is generated from the C++/Python builder using
`tests/generator/test_additional_atomic_segments` with `--only-generator` and
`--serialization-method V2`.

`test-mixed-case-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_mixed_case` with `--only-analyzer` and
`--serialization-method V2`.

`test-names-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_names` with `--only-analyzer` and
`--serialization-method V2`.

`test-names-s.dict` is generated from the C++/Python builder using
`tests/generator/test_names` with `--only-generator` and
`--serialization-method V2`.

`test-prefixes-uppercase-beginning-a.dict` is generated from the C++/Python
builder using `tests/analyzer/test_prefixes_with_uppercase_at_the_beginning`
with `--only-analyzer` and `--serialization-method V2`.

`test-prefixes-uppercase-middle-a.dict` is generated from the C++/Python
builder using `tests/analyzer/test_prefixes_with_uppercase_in_the_middle` with
`--only-analyzer` and `--serialization-method V2`.

`test-qualifiers-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_qualifiers` with `--only-analyzer` and
`--serialization-method V2`.

`test-qualifiers-s.dict` is generated from the C++/Python builder using
`tests/generator/test_qualifiers` with `--only-generator` and
`--serialization-method V2`.

`test-segtypes-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_segtypes` with `--only-analyzer` and
`--serialization-method V2`.

`test-segtypes-homonyms-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_segtypes_with_homonyms` with `--only-analyzer` and
`--serialization-method V2`.

`test-segtypes-s.dict` is generated from the C++/Python builder using
`tests/generator/test_segtypes` with `--only-generator` and
`--serialization-method V2`.

Binary dictionary labels are compared in the canonical order written by
`serializeQualifiersMap`, which may differ from the order preserved in source
fixture `output.txt` files.

The builder currently needs Python `pyparsing` available on `PYTHONPATH`.

`test-multisegments-a.dict` is generated from the C++/Python builder using
`tests/analyzer/test_multisegments` with `--only-analyzer` and
`--serialization-method V2`. It exercises hyphenated compound segmentation
(`biało-czerwony` -> adja + interp + adj through the inflexion graph).

`test-digits-a.dict` and `test-whitespace-append-a.dict` are generated from the
C++/Python builder using `tests/analyzer/test_digits` and
`tests/analyzer/test_whitespace_handling_append` respectively, with
`--only-analyzer` and `--serialization-method V2`.
