use std::fs;
use std::process::Command;

#[test]
fn c_header_matches_legacy_surface() {
    let source = r#"
#include "morfeusz2_c.h"

#ifndef __MORFEUSZ_H__
#error legacy C include guard macro is missing
#endif
#ifndef MORFEUSZ2_C_H
#error Rust C include guard macro is missing
#endif

#if MORFOPT_ENCODING != 1
#error MORFOPT_ENCODING mismatch
#endif
#if MORFEUSZ_UTF_8 != 8
#error MORFEUSZ_UTF_8 mismatch
#endif
#if MORFEUSZ_ISO8859_2 != 88592
#error MORFEUSZ_ISO8859_2 mismatch
#endif
#if MORFEUSZ_CP1250 != 1250
#error MORFEUSZ_CP1250 mismatch
#endif
#if MORFEUSZ_CP852 != 852
#error MORFEUSZ_CP852 mismatch
#endif
#if MORFOPT_WHITESPACE != 2
#error MORFOPT_WHITESPACE mismatch
#endif
#if MORFEUSZ_SKIP_WHITESPACE != 0
#error MORFEUSZ_SKIP_WHITESPACE mismatch
#endif
#if MORFEUSZ_KEEP_WHITESPACE != 2
#error MORFEUSZ_KEEP_WHITESPACE mismatch
#endif
#if MORFEUSZ_APPEND_WHITESPACE != 4
#error MORFEUSZ_APPEND_WHITESPACE mismatch
#endif
#if MORFOPT_CASE != 3
#error MORFOPT_CASE mismatch
#endif
#if MORFEUSZ_WEAK_CASE != 301
#error MORFEUSZ_WEAK_CASE mismatch
#endif
#if MORFEUSZ_STRICT_CASE != 302
#error MORFEUSZ_STRICT_CASE mismatch
#endif
#if MORFEUSZ_IGNORE_CASE != 303
#error MORFEUSZ_IGNORE_CASE mismatch
#endif
#if MORFOPT_TOKEN_NUMBERING != 4
#error MORFOPT_TOKEN_NUMBERING mismatch
#endif
#if MORFEUSZ_SEPARATE_TOKEN_NUMBERING != 401
#error MORFEUSZ_SEPARATE_TOKEN_NUMBERING mismatch
#endif
#if MORFEUSZ_CONTINUOUS_TOKEN_NUMBERING != 402
#error MORFEUSZ_CONTINUOUS_TOKEN_NUMBERING mismatch
#endif

static char *(*about_ptr)(void) = morfeusz_about;
static char *(*default_dict_name_ptr)(void) = morfeusz_get_default_dict_name;
static char *(*copyright_ptr)(void) = morfeusz_get_copyright;
static const char *(*last_error_ptr)(void) = morfeusz_last_error;
static InterpMorf *(*analyse_ptr)(char *) = morfeusz_analyse;
static int (*set_option_ptr)(int, int) = morfeusz_set_option;
static void (*paths_clear_ptr)(void) = morfeusz_dictionary_search_paths_clear;
static int (*paths_push_ptr)(const char *) = morfeusz_dictionary_search_paths_push;
static size_t (*paths_count_ptr)(void) = morfeusz_dictionary_search_paths_count;
static const char *(*paths_item_ptr)(size_t) = morfeusz_dictionary_search_paths_item;

static MorfeuszInstance *(*create_ptr)(int) = morfeusz_create_instance;
static MorfeuszInstance *(*create_named_ptr)(const char *, int) = morfeusz_create_instance_named;
static MorfeuszInstance *(*clone_ptr)(const MorfeuszInstance *) = morfeusz_clone_instance;
static void (*destroy_ptr)(MorfeuszInstance *) = morfeusz_destroy_instance;
static MorfeuszOwnedInterps *(*instance_analyse_ptr)(MorfeuszInstance *, const char *) = morfeusz_instance_analyse;
static MorfeuszOwnedInterps *(*instance_generate_ptr)(MorfeuszInstance *, const char *) = morfeusz_instance_generate;
static MorfeuszOwnedInterps *(*instance_generate_tag_ptr)(MorfeuszInstance *, const char *, int) = morfeusz_instance_generate_by_tag_id;
static size_t (*interps_len_ptr)(const MorfeuszOwnedInterps *) = morfeusz_interps_len;
static const MorfeuszInterp *(*interps_data_ptr)(const MorfeuszOwnedInterps *) = morfeusz_interps_data;
static void (*destroy_interps_ptr)(MorfeuszOwnedInterps *) = morfeusz_destroy_interps;

int main(void) {
  InterpMorf interp = {0, 1, 0, 0, 0};
  MorfeuszInterp rich = {0, 1, "orth", "lemma", 2, 3, 4};
  return interp.k - interp.p - 1 + rich.endNode - rich.startNode - 1;
}
"#;

    compile_syntax("cc", "c", &["-std=c99"], source, "C header contract");
}

#[test]
fn cxx_header_exposes_legacy_constants_and_value_type() {
    let source = r#"
#include "morfeusz2.h"

#if MORFEUSZ_UTF_8 != 8
#error C compatibility constants are missing from morfeusz2.h
#endif

using namespace morfeusz;

static_assert(UTF8 == 11, "UTF8 mismatch");
static_assert(ISO8859_2 == 12, "ISO8859_2 mismatch");
static_assert(CP1250 == 13, "CP1250 mismatch");
static_assert(CP852 == 14, "CP852 mismatch");

static_assert(CONDITIONALLY_CASE_SENSITIVE == 100, "case mismatch");
static_assert(STRICTLY_CASE_SENSITIVE == 101, "case mismatch");
static_assert(IGNORE_CASE == 102, "case mismatch");

static_assert(SEPARATE_NUMBERING == 201, "numbering mismatch");
static_assert(CONTINUOUS_NUMBERING == 202, "numbering mismatch");

static_assert(SKIP_WHITESPACES == 301, "whitespace mismatch");
static_assert(APPEND_WHITESPACES == 302, "whitespace mismatch");
static_assert(KEEP_WHITESPACES == 303, "whitespace mismatch");

static_assert(ANALYSE_ONLY == 401, "usage mismatch");
static_assert(GENERATE_ONLY == 402, "usage mismatch");
static_assert(BOTH_ANALYSE_AND_GENERATE == 403, "usage mismatch");

struct CustomResolver : IdResolver {
  const std::string getTagsetId() const override { return "custom"; }
  const std::string& getTag(const int) const override {
    static const std::string value("tag");
    return value;
  }
  int getTagId(const std::string&) const override { return 0; }
  const std::string& getName(const int) const override {
    static const std::string value("name");
    return value;
  }
  int getNameId(const std::string&) const override { return 0; }
  const std::string& getLabelsAsString(int) const override {
    static const std::string value("_");
    return value;
  }
  const std::set<std::string>& getLabels(int) const override {
    static const std::set<std::string> value;
    return value;
  }
  int getLabelsId(const std::string&) const override { return 0; }
  size_t getTagsCount() const override { return 1; }
  size_t getNamesCount() const override { return 1; }
  size_t getLabelsCount() const override { return 1; }
};

struct CustomMorfeusz : Morfeusz {
  CustomMorfeusz() : Morfeusz(), resolver() {}
  std::string getDictID() const override { return "dict"; }
  std::string getDictCopyright() const override { return "copyright"; }
  Morfeusz* clone() const override { return new CustomMorfeusz(); }
  ResultsIterator* analyse(const std::string&) const override {
    std::vector<MorphInterpretation> out;
    return new ResultsIterator(out);
  }
  ResultsIterator* analyse(const char*) const override {
    std::vector<MorphInterpretation> out;
    return new ResultsIterator(out);
  }
  void analyse(const std::string&, std::vector<MorphInterpretation>&) const override {}
  void generate(const std::string&, std::vector<MorphInterpretation>&) const override {}
  void generate(const std::string&, int, std::vector<MorphInterpretation>&) const override {}
  void setCharset(Charset) override {}
  Charset getCharset() const override { return UTF8; }
  void setAggl(const std::string&) override {}
  std::string getAggl() const override { return "isolated"; }
  void setPraet(const std::string&) override {}
  std::string getPraet() const override { return "split"; }
  void setCaseHandling(CaseHandling) override {}
  CaseHandling getCaseHandling() const override { return CONDITIONALLY_CASE_SENSITIVE; }
  void setTokenNumbering(TokenNumbering) override {}
  TokenNumbering getTokenNumbering() const override { return SEPARATE_NUMBERING; }
  void setWhitespaceHandling(WhitespaceHandling) override {}
  WhitespaceHandling getWhitespaceHandling() const override { return SKIP_WHITESPACES; }
  void setDebug(bool) override {}
  const IdResolver& getIdResolver() const override { return resolver; }
  void setDictionary(const std::string&) override {}
  const std::set<std::string>& getAvailableAgglOptions() const override { return options; }
  const std::set<std::string>& getAvailablePraetOptions() const override { return options; }

protected:
  ResultsIterator* analyseWithCopy(const char*) const override {
    std::vector<MorphInterpretation> out;
    return new ResultsIterator(out);
  }

private:
  CustomResolver resolver;
  std::set<std::string> options;
};

struct CustomIterator : ResultsIterator {
  CustomIterator() : ResultsIterator(), value() {}
  bool hasNext() override { return false; }
  const MorphInterpretation& peek() override { return value; }
  MorphInterpretation next() override { return value; }

private:
  MorphInterpretation value;
};

int main() {
  MorphInterpretation ign = MorphInterpretation::createIgn(3, 4, "orth", "lemma");
  if (!ign.isIgn() || ign.isWhitespace()) return 1;
  if (ign.startNode != 3 || ign.endNode != 4) return 2;
  if (ign.orth != "orth" || ign.lemma != "lemma") return 3;

  MorphInterpretation ws = MorphInterpretation::createWhitespace(5, 6, "  ");
  if (!ws.isWhitespace() || ws.isIgn()) return 4;
  if (ws.orth != "  " || ws.lemma != "  ") return 5;

  MorfeuszException err = std::string("message");
  FileFormatException format_err = std::string("format");
  std::vector<MorphInterpretation> out;
  ResultsIterator iterator(out);
  Morfeusz* morf = 0;
  IdResolver* resolver = 0;
  CustomResolver custom_resolver;
  resolver = &custom_resolver;
  if (resolver->getTagsetId() != "custom") return 10;
  (void) resolver;
  CustomMorfeusz custom_morfeusz;
  if (custom_morfeusz.getDictID() != "dict") return 11;
  CustomIterator custom_iterator;
  if (custom_iterator.hasNext()) return 12;
  Morfeusz* (*create_default)(MorfeuszUsage) = &Morfeusz::createInstance;
  Morfeusz* (*create_named)(const std::string&, MorfeuszUsage) = &Morfeusz::createInstance;
  (void) create_default;
  (void) create_named;
  Morfeusz::dictionarySearchPaths.push_back(".");
  Morfeusz::dictionarySearchPaths.clear();
  if (morf != 0) {
    morf->analyse("7", out);
    morf->generate("7", out);
    morf->generate("7", 151, out);
    morf->setCharset(UTF8);
    morf->setAggl("isolated");
    morf->setPraet("split");
    morf->setCaseHandling(CONDITIONALLY_CASE_SENSITIVE);
    morf->setTokenNumbering(SEPARATE_NUMBERING);
    morf->setWhitespaceHandling(SKIP_WHITESPACES);
    morf->setDebug(false);
    morf->getDictID();
    morf->getDictCopyright();
    morf->getIdResolver().getTag(0);
    morf->getAvailableAgglOptions();
    morf->getAvailablePraetOptions();
  }
  if (iterator.hasNext()) return 9;
  if (Morfeusz::getVersion().empty()) return 6;
  if (Morfeusz::getDefaultDictName().empty()) return 7;
  if (Morfeusz::getCopyright().empty()) return 8;
  return std::string(err.what()) == "message" ? 0 : 6;
}
"#;

    compile_syntax("c++", "cc", &["-std=c++11"], source, "C++ header contract");
}

fn compile_syntax(compiler: &str, extension: &str, extra_args: &[&str], source: &str, label: &str) {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let source_path = std::env::temp_dir().join(format!(
        "morfeusz2_header_contract_{}.{}",
        std::process::id(),
        extension
    ));
    fs::write(&source_path, source).unwrap();

    let include_dir = format!("{manifest_dir}/include");
    let mut command = Command::new(compiler);
    command.args(extra_args);
    command
        .arg("-fsyntax-only")
        .arg("-I")
        .arg(include_dir)
        .arg(source_path.to_str().unwrap());
    let output = match command.output() {
        Ok(output) => output,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping {label} because {compiler} is not available");
            let _ = fs::remove_file(&source_path);
            return;
        }
        Err(err) => panic!("failed to execute {compiler}: {err}"),
    };
    let _ = fs::remove_file(&source_path);

    assert!(
        output.status.success(),
        "{label} failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
