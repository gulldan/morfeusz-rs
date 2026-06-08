use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[test]
fn c_client_links_against_rust_c_api_library() {
    let cc = match Command::new("cc").arg("--version").output() {
        Ok(_) => "cc",
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping C link smoke test because cc is not available");
            return;
        }
        Err(err) => panic!("failed to execute cc: {err}"),
    };

    build_c_api_library();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = target_profile_dir();
    let library = target_dir.join(shared_library_name());
    assert!(
        library.exists(),
        "expected C API shared library at {}",
        library.display()
    );

    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join("sgjp-a.dict"),
    )
    .unwrap();
    fs::copy(
        fixture("test-digits-v1-s.dict"),
        temp_dir.join("sgjp-s.dict"),
    )
    .unwrap();
    fs::copy(fixture("test-names-a.dict"), temp_dir.join("names-a.dict")).unwrap();
    fs::copy(
        fixture("test-qualifiers-a.dict"),
        temp_dir.join("qualifiers-a.dict"),
    )
    .unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join("switch-a.dict"),
    )
    .unwrap();
    fs::copy(
        fixture("test-digits-v1-s.dict"),
        temp_dir.join("switch-s.dict"),
    )
    .unwrap();
    let source_path = temp_dir.join("client.c");
    let exe_path = temp_dir.join("client");
    fs::write(
        &source_path,
        r#"
#include "morfeusz2_c.h"
#include <stddef.h>
#include <string.h>

enum {
  ANALYSE_ONLY = 401,
  BOTH_ANALYSE_AND_GENERATE = 403,
  UTF8 = 11,
  CP1250 = 13,
  CONDITIONALLY_CASE_SENSITIVE = 100,
  IGNORE_CASE = 102,
  SEPARATE_NUMBERING = 201,
  CONTINUOUS_NUMBERING = 202,
  SKIP_WHITESPACES = 301,
  KEEP_WHITESPACES = 303
};

int main(void) {
  char text[] = "7";
  InterpMorf *result;
  MorfeuszInstance *instance;
  MorfeuszInstance *clone;
  MorfeuszInstance *names;
  MorfeuszInstance *qualifiers;
  MorfeuszOwnedInterps *owned;
  const MorfeuszInterp *items;
  size_t len;
  size_t i;
  int dig;
  int saw_digit;
  int saw_named;
  int saw_labels;

  if (morfeusz_about() == 0) return 2;
  if (morfeusz_get_default_dict_name() == 0) return 11;
  if (strcmp(morfeusz_get_default_dict_name(), "sgjp") != 0) return 12;
  if (morfeusz_get_copyright() == 0) return 13;
  if (morfeusz_last_error() == 0) return 10;
  if (!morfeusz_set_option(MORFOPT_ENCODING, MORFEUSZ_UTF_8)) return 3;
  if (morfeusz_set_option(MORFOPT_ENCODING, 666777) != 0) return 75;
  if (morfeusz_set_option(MORFOPT_WHITESPACE, 666777) != 0) return 76;
  if (morfeusz_set_option(MORFOPT_CASE, 666777) != 0) return 77;
  if (morfeusz_set_option(MORFOPT_TOKEN_NUMBERING, 666777) != 0) return 78;
  if (morfeusz_set_option(666777, 1) != 0) return 79;
  if (!morfeusz_set_option(MORFOPT_ENCODING, MORFEUSZ_UTF_8)) return 80;

  result = morfeusz_analyse(text);
  if (result == 0) return 4;
  if (result[0].p != 0 || result[0].k != 1) return 5;
  if (strcmp(result[0].forma, "7") != 0) return 6;
  if (strcmp(result[0].haslo, "7") != 0) return 7;
  if (strcmp(result[0].interp, "dig") != 0) return 8;
  if (result[1].p != -1) return 9;

  morfeusz_dictionary_search_paths_clear();
  if (morfeusz_dictionary_search_paths_count() != 0) return 14;
  if (!morfeusz_dictionary_search_paths_push(".")) return 15;
  if (morfeusz_dictionary_search_paths_count() != 1) return 16;
  if (strcmp(morfeusz_dictionary_search_paths_item(0), ".") != 0) return 17;

  instance = morfeusz_create_instance(BOTH_ANALYSE_AND_GENERATE);
  if (instance == 0) return 18;
  if (morfeusz_instance_last_error(instance) == 0) return 19;
  if (morfeusz_instance_get_dict_id(instance) == 0) return 20;
  if (morfeusz_instance_get_dict_copyright(instance) == 0) return 21;

  if (morfeusz_instance_get_charset(instance) != UTF8) return 22;
  if (!morfeusz_instance_set_charset(instance, CP1250)) return 23;
  if (morfeusz_instance_get_charset(instance) != CP1250) return 24;
  if (!morfeusz_instance_set_charset(instance, UTF8)) return 25;
  if (!morfeusz_instance_set_case_handling(instance, IGNORE_CASE)) return 26;
  if (morfeusz_instance_get_case_handling(instance) != IGNORE_CASE) return 27;
  if (!morfeusz_instance_set_case_handling(instance, CONDITIONALLY_CASE_SENSITIVE)) return 28;
  if (!morfeusz_instance_set_token_numbering(instance, CONTINUOUS_NUMBERING)) return 29;
  if (morfeusz_instance_get_token_numbering(instance) != CONTINUOUS_NUMBERING) return 30;
  if (!morfeusz_instance_set_token_numbering(instance, SEPARATE_NUMBERING)) return 31;
  if (!morfeusz_instance_set_whitespace_handling(instance, KEEP_WHITESPACES)) return 32;
  if (morfeusz_instance_get_whitespace_handling(instance) != KEEP_WHITESPACES) return 33;
  if (!morfeusz_instance_set_whitespace_handling(instance, SKIP_WHITESPACES)) return 34;
  if (morfeusz_instance_available_aggl_count(instance) == 0) return 35;
  if (morfeusz_instance_available_aggl_item(instance, 0) == 0) return 36;
  if (morfeusz_instance_available_praet_count(instance) == 0) return 37;
  if (morfeusz_instance_available_praet_item(instance, 0) == 0) return 38;
  if (!morfeusz_instance_set_aggl(instance, morfeusz_instance_get_aggl(instance))) return 39;
  if (!morfeusz_instance_set_praet(instance, morfeusz_instance_get_praet(instance))) return 40;
  morfeusz_instance_set_debug(instance, 1);
  morfeusz_instance_set_debug(instance, 0);

  owned = morfeusz_instance_analyse(instance, "7");
  if (owned == 0) return 41;
  len = morfeusz_interps_len(owned);
  items = morfeusz_interps_data(owned);
  saw_digit = 0;
  for (i = 0; i < len; ++i) {
    if (items[i].orth != 0 && items[i].lemma != 0 &&
        strcmp(items[i].orth, "7") == 0 && strcmp(items[i].lemma, "7") == 0) {
      saw_digit = 1;
    }
  }
  morfeusz_destroy_interps(owned);
  if (!saw_digit) return 42;

  dig = morfeusz_instance_get_tag_id(instance, "dig");
  if (dig < 0) return 43;
  if (strcmp(morfeusz_instance_get_tag(instance, dig), "dig") != 0) return 44;
  if (morfeusz_instance_get_tags_count(instance) == 0) return 45;

  owned = morfeusz_instance_generate(instance, "123");
  if (owned == 0) return 46;
  len = morfeusz_interps_len(owned);
  items = morfeusz_interps_data(owned);
  saw_digit = 0;
  for (i = 0; i < len; ++i) {
    if (items[i].orth != 0 && strcmp(items[i].orth, "123") == 0 &&
        items[i].tagId == dig) {
      saw_digit = 1;
    }
  }
  morfeusz_destroy_interps(owned);
  if (!saw_digit) return 47;

  owned = morfeusz_instance_generate_by_tag_id(instance, "123", dig);
  if (owned == 0) return 48;
  if (morfeusz_interps_len(owned) == 0) return 49;
  morfeusz_destroy_interps(owned);
  owned = morfeusz_instance_generate_by_tag_id(
      instance, "123", (int)morfeusz_instance_get_tags_count(instance));
  if (owned != 0) return 73;
  if (morfeusz_instance_last_error(instance) == 0 ||
      strstr(morfeusz_instance_last_error(instance), "Invalid tag") == 0) return 74;

  if (!morfeusz_instance_set_whitespace_handling(instance, KEEP_WHITESPACES)) return 50;
  clone = morfeusz_clone_instance(instance);
  if (clone == 0) return 51;
  if (morfeusz_instance_get_whitespace_handling(clone) != KEEP_WHITESPACES) return 52;
  morfeusz_destroy_instance(clone);

  if (!morfeusz_instance_set_dictionary(instance, "switch")) return 65;
  if (morfeusz_instance_get_whitespace_handling(instance) != KEEP_WHITESPACES) return 66;
  owned = morfeusz_instance_analyse(instance, "7 7");
  if (owned == 0) return 67;
  len = morfeusz_interps_len(owned);
  items = morfeusz_interps_data(owned);
  saw_digit = 0;
  for (i = 0; i < len; ++i) {
    if (items[i].orth != 0 && strcmp(items[i].orth, "7") == 0 &&
        items[i].tagId == dig) {
      saw_digit = 1;
    }
  }
  morfeusz_destroy_interps(owned);
  if (!saw_digit) return 68;
  owned = morfeusz_instance_generate(instance, "123");
  if (owned == 0) return 69;
  len = morfeusz_interps_len(owned);
  items = morfeusz_interps_data(owned);
  saw_digit = 0;
  for (i = 0; i < len; ++i) {
    if (items[i].orth != 0 && strcmp(items[i].orth, "123") == 0 &&
        items[i].tagId == dig) {
      saw_digit = 1;
    }
  }
  morfeusz_destroy_interps(owned);
  if (!saw_digit) return 70;

  morfeusz_destroy_instance(instance);

  names = morfeusz_create_instance_named("names", ANALYSE_ONLY);
  if (names == 0) return 53;
  if (!morfeusz_instance_set_charset(names, CP1250)) return 71;
  owned = morfeusz_instance_analyse(names, "czerwony");
  if (owned == 0) return 54;
  len = morfeusz_interps_len(owned);
  items = morfeusz_interps_data(owned);
  saw_named = 0;
  for (i = 0; i < len; ++i) {
    if (items[i].lemma != 0 && strcmp(items[i].lemma, "czerwony:a3") == 0) {
      const char *name = morfeusz_instance_get_name(names, items[i].nameId);
      if (name == 0 || name[0] == '\0' || strcmp(name, "_") == 0) return 55;
      if (morfeusz_instance_get_name_id(names, name) != items[i].nameId) return 56;
      saw_named = 1;
    }
  }
  morfeusz_destroy_interps(owned);
  if (!saw_named) return 57;
  if (morfeusz_instance_get_names_count(names) == 0) return 58;
  morfeusz_destroy_instance(names);

  qualifiers = morfeusz_create_instance_named("qualifiers", ANALYSE_ONLY);
  if (qualifiers == 0) return 59;
  if (!morfeusz_instance_set_charset(qualifiers, CP1250)) return 72;
  owned = morfeusz_instance_analyse(qualifiers, "czerwony");
  if (owned == 0) return 60;
  len = morfeusz_interps_len(owned);
  items = morfeusz_interps_data(owned);
  saw_labels = 0;
  for (i = 0; i < len; ++i) {
    if (items[i].lemma != 0 && strcmp(items[i].lemma, "czerwony:a4") == 0) {
      const char *labels = morfeusz_instance_get_labels_as_string(qualifiers, items[i].labelsId);
      if (labels == 0 || labels[0] == '\0' || strcmp(labels, "_") == 0) return 61;
      if (morfeusz_instance_get_labels_id(qualifiers, labels) != items[i].labelsId) return 62;
      saw_labels = 1;
    }
  }
  morfeusz_destroy_interps(owned);
  if (!saw_labels) return 63;
  if (morfeusz_instance_get_labels_count(qualifiers) == 0) return 64;
  morfeusz_destroy_instance(qualifiers);

  return 0;
}
"#,
    )
    .unwrap();

    let compile = Command::new(cc)
        .args([
            "-std=c99",
            "-I",
            manifest_dir.join("include").to_str().unwrap(),
            source_path.to_str().unwrap(),
            "-L",
            target_dir.to_str().unwrap(),
            "-lmorfeusz2",
            "-o",
            exe_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "C client compile/link failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&exe_path)
        .current_dir(&temp_dir)
        .env(dynamic_library_path_env(), &target_dir)
        .output()
        .unwrap();
    fs::remove_dir_all(temp_dir).unwrap();

    assert!(
        run.status.success(),
        "C client exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
    let stderr = String::from_utf8_lossy(&run.stderr);
    assert!(
        stderr.contains("Wrong encoding option 666777"),
        "missing legacy encoding diagnostic in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Wrong whitespace option 666777"),
        "missing legacy whitespace diagnostic in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Wrong case option 666777"),
        "missing legacy case/token diagnostic in stderr:\n{stderr}"
    );
    assert!(
        stderr.contains("Wrong option 666777"),
        "missing legacy option diagnostic in stderr:\n{stderr}"
    );
}

#[test]
fn cxx_client_links_legacy_morfeusz2_header_static_getters() {
    let cxx = match Command::new("c++").arg("--version").output() {
        Ok(_) => "c++",
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping C++ link smoke test because c++ is not available");
            return;
        }
        Err(err) => panic!("failed to execute c++: {err}"),
    };

    build_c_api_library();

    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target_dir = target_profile_dir();
    let library = target_dir.join(shared_library_name());
    assert!(
        library.exists(),
        "expected C API shared library at {}",
        library.display()
    );

    let temp_dir = unique_temp_dir();
    fs::create_dir_all(&temp_dir).unwrap();
    fs::copy(fixture("test-digits-a.dict"), temp_dir.join("sgjp-a.dict")).unwrap();
    fs::copy(fixture("test-digits-s.dict"), temp_dir.join("sgjp-s.dict")).unwrap();
    fs::copy(fixture("test-names-a.dict"), temp_dir.join("names-a.dict")).unwrap();
    fs::copy(
        fixture("test-qualifiers-a.dict"),
        temp_dir.join("qualifiers-a.dict"),
    )
    .unwrap();
    fs::copy(
        fixture("test-dict-copyright-v1-a.dict"),
        temp_dir.join("switch-a.dict"),
    )
    .unwrap();
    fs::copy(
        fixture("test-digits-v1-s.dict"),
        temp_dir.join("switch-s.dict"),
    )
    .unwrap();
    let source_path = temp_dir.join("client.cc");
    let exe_path = temp_dir.join("client-cxx");
    let dict_dir_literal = temp_dir
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"");
    let source = r#"
#include "morfeusz2.h"
#include <vector>

int main() {
  if (morfeusz::Morfeusz::getVersion() != "1.99.15") return 1;
  if (morfeusz::Morfeusz::getDefaultDictName() != "sgjp") return 2;
  if (morfeusz::Morfeusz::getCopyright().find("Copyright") != 0) return 3;

  morfeusz::Morfeusz *default_morf =
      morfeusz::Morfeusz::createInstance(morfeusz::BOTH_ANALYSE_AND_GENERATE);
  if (default_morf == 0) return 30;
  delete default_morf;

  bool empty_paths_failed = false;
  std::string empty_paths_error;
  morfeusz::Morfeusz::dictionarySearchPaths.clear();
  try {
    morfeusz::Morfeusz *missing =
        morfeusz::Morfeusz::createInstance(morfeusz::BOTH_ANALYSE_AND_GENERATE);
    delete missing;
  } catch (const morfeusz::MorfeuszException& err) {
    empty_paths_failed = true;
    empty_paths_error = err.what();
  }
  if (!empty_paths_failed) return 4;
  if (empty_paths_error.find("Failed to load analyzer dictionary") == std::string::npos)
    return 41;

  morfeusz::Morfeusz::dictionarySearchPaths.push_back("__DICT_DIR__");
  morfeusz::Morfeusz *morf =
      morfeusz::Morfeusz::createInstance(morfeusz::BOTH_ANALYSE_AND_GENERATE);
  if (morf == 0) return 40;
  morfeusz::Morfeusz *named_sgjp =
      morfeusz::Morfeusz::createInstance(std::string("sgjp"),
                                         morfeusz::BOTH_ANALYSE_AND_GENERATE);
  if (named_sgjp == 0) return 42;
  delete named_sgjp;
  if (morf->getCharset() != morfeusz::UTF8) return 5;
  if (morf->getCaseHandling() != morfeusz::CONDITIONALLY_CASE_SENSITIVE) return 6;
  if (morf->getTokenNumbering() != morfeusz::SEPARATE_NUMBERING) return 7;
  if (morf->getWhitespaceHandling() != morfeusz::SKIP_WHITESPACES) return 8;
  morf->setCharset(morfeusz::CP1250);
  if (morf->getCharset() != morfeusz::CP1250) return 43;
  morf->setCharset(morfeusz::UTF8);
  morf->setCaseHandling(morfeusz::IGNORE_CASE);
  if (morf->getCaseHandling() != morfeusz::IGNORE_CASE) return 44;
  morf->setTokenNumbering(morfeusz::CONTINUOUS_NUMBERING);
  if (morf->getTokenNumbering() != morfeusz::CONTINUOUS_NUMBERING) return 45;
  morf->setTokenNumbering(morfeusz::SEPARATE_NUMBERING);
  morf->getDictID();
  morf->getDictCopyright();
  const std::string current_aggl = morf->getAggl();
  const std::string current_praet = morf->getPraet();
  const std::set<std::string>& aggl_options = morf->getAvailableAgglOptions();
  const size_t aggl_options_size = aggl_options.size();
  if (aggl_options.find(current_aggl) == aggl_options.end()) return 46;
  const std::set<std::string>& praet_options = morf->getAvailablePraetOptions();
  if (praet_options.find(current_praet) == praet_options.end()) return 47;
  if (&aggl_options == &praet_options) return 74;
  if (aggl_options.find(current_aggl) == aggl_options.end()) return 75;
  if (aggl_options.size() != aggl_options_size) return 76;
  morf->setAggl(current_aggl);
  morf->setPraet(current_praet);

  std::vector<morfeusz::MorphInterpretation> analyzed;
  morf->analyse("7", analyzed);
  bool saw_digit = false;
  for (size_t i = 0; i < analyzed.size(); ++i) {
    if (analyzed[i].orth == "7" && analyzed[i].lemma == "7" &&
        analyzed[i].getTag(*morf) == "dig") {
      saw_digit = true;
    }
  }
  if (!saw_digit) return 11;

  morfeusz::ResultsIterator *it = morf->analyse("7");
  if (it == 0 || !it->hasNext()) return 12;
  const morfeusz::MorphInterpretation& peeked = it->peek();
  if (peeked.orth != "7") return 48;
  morfeusz::MorphInterpretation first = it->next();
  if (it->hasNext()) return 49;
  delete it;
  if (first.orth != "7") return 13;

  const int dig = morf->getIdResolver().getTagId("dig");
  if (morf->getIdResolver().getTag(dig) != "dig") return 50;
  const std::string& dig_ref = morf->getIdResolver().getTag(dig);
  const std::string& ign_ref = morf->getIdResolver().getTag(0);
  if (ign_ref != "ign") return 69;
  if (dig_ref != "dig") return 70;
  if (morf->getIdResolver().getTagsCount() == 0) return 51;
  std::vector<morfeusz::MorphInterpretation> generated;
  morf->generate("7", generated);
  saw_digit = false;
  for (size_t i = 0; i < generated.size(); ++i) {
    if (generated[i].orth == "7" && generated[i].lemma == "7" &&
        generated[i].tagId == dig) {
      saw_digit = true;
    }
  }
  if (!saw_digit) return 52;
  generated.clear();
  morf->generate("7", dig, generated);
  saw_digit = false;
  for (size_t i = 0; i < generated.size(); ++i) {
    if (generated[i].orth == "7" && generated[i].lemma == "7" &&
        generated[i].tagId == dig) {
      saw_digit = true;
    }
  }
  if (!saw_digit) return 14;
  bool invalid_tag_failed = false;
  try {
    generated.clear();
    morf->generate("7", (int)morf->getIdResolver().getTagsCount(), generated);
  } catch (const morfeusz::MorfeuszException& err) {
    invalid_tag_failed = true;
    if (std::string(err.what()).find("Invalid tag") == std::string::npos) return 77;
  }
  if (!invalid_tag_failed) return 78;

  morf->setWhitespaceHandling(morfeusz::KEEP_WHITESPACES);
  if (morf->getWhitespaceHandling() != morfeusz::KEEP_WHITESPACES) return 15;
  morfeusz::Morfeusz *clone = morf->clone();
  if (clone == 0) return 16;
  if (clone->getWhitespaceHandling() != morfeusz::KEEP_WHITESPACES) return 53;
  delete clone;

  morf->setDictionary("switch");
  if (morf->getWhitespaceHandling() != morfeusz::KEEP_WHITESPACES) return 64;
  if (morf->getCaseHandling() != morfeusz::IGNORE_CASE) return 65;
  analyzed.clear();
  morf->analyse("7  7", analyzed);
  saw_digit = false;
  bool saw_whitespace = false;
  for (size_t i = 0; i < analyzed.size(); ++i) {
    if (analyzed[i].orth == "7" && analyzed[i].lemma == "7" &&
        analyzed[i].tagId == dig) {
      saw_digit = true;
    }
    if (analyzed[i].isWhitespace() && analyzed[i].orth == "  ") {
      saw_whitespace = true;
    }
  }
  if (!saw_digit) return 66;
  if (!saw_whitespace) return 67;
  generated.clear();
  morf->generate("123", generated);
  saw_digit = false;
  for (size_t i = 0; i < generated.size(); ++i) {
    if (generated[i].orth == "123" && generated[i].lemma == "123" &&
        generated[i].tagId == dig) {
      saw_digit = true;
    }
  }
  if (!saw_digit) return 68;

  morfeusz::Morfeusz *names =
      morfeusz::Morfeusz::createInstance("names", morfeusz::ANALYSE_ONLY);
  if (names == 0) return 54;
  names->setCharset(morfeusz::CP1250);
  std::vector<morfeusz::MorphInterpretation> named_analyzed;
  names->analyse("czerwony", named_analyzed);
  bool saw_named = false;
  for (size_t i = 0; i < named_analyzed.size(); ++i) {
    if (named_analyzed[i].lemma == "czerwony:a3") {
      const std::string& name = named_analyzed[i].getName(*names);
      if (name.empty() || name == "_") return 55;
      const std::string saved_name = name;
      const std::string& empty_name = names->getIdResolver().getName(0);
      (void) empty_name;
      if (name != saved_name) return 71;
      if (names->getIdResolver().getNameId(name) != named_analyzed[i].nameId) return 56;
      saw_named = true;
    }
  }
  delete names;
  if (!saw_named) return 57;

  morfeusz::Morfeusz *qualifiers =
      morfeusz::Morfeusz::createInstance("qualifiers", morfeusz::ANALYSE_ONLY);
  if (qualifiers == 0) return 58;
  qualifiers->setCharset(morfeusz::CP1250);
  std::vector<morfeusz::MorphInterpretation> qualifier_analyzed;
  qualifiers->analyse("czerwony", qualifier_analyzed);
  bool saw_labels = false;
  for (size_t i = 0; i < qualifier_analyzed.size(); ++i) {
    if (qualifier_analyzed[i].lemma == "czerwony:a4") {
      const std::string& labels = qualifier_analyzed[i].getLabelsAsString(*qualifiers);
      if (labels.empty() || labels == "_") return 59;
      const std::string saved_labels = labels;
      const std::string& empty_labels_string = qualifiers->getIdResolver().getLabelsAsString(0);
      (void) empty_labels_string;
      if (labels != saved_labels) return 72;
      if (qualifiers->getIdResolver().getLabelsId(labels) !=
          qualifier_analyzed[i].labelsId) return 60;
      const std::set<std::string>& labels_set = qualifier_analyzed[i].getLabels(*qualifiers);
      if (labels_set.size() != 3) return 61;
      const std::set<std::string>& empty_labels = qualifiers->getIdResolver().getLabels(0);
      (void) empty_labels;
      if (labels_set.size() != 3) return 73;
      if (qualifiers->getIdResolver().getLabelsCount() == 0) return 62;
      saw_labels = true;
    }
  }
  delete qualifiers;
  if (!saw_labels) return 63;

  delete morf;
  return 0;
}
"#
    .replace("__DICT_DIR__", &dict_dir_literal);
    fs::write(&source_path, source).unwrap();

    let compile = Command::new(cxx)
        .args([
            "-std=c++11",
            "-I",
            manifest_dir.join("include").to_str().unwrap(),
            source_path.to_str().unwrap(),
            "-L",
            target_dir.to_str().unwrap(),
            "-lmorfeusz2",
            "-o",
            exe_path.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        compile.status.success(),
        "C++ client compile/link failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&compile.stdout),
        String::from_utf8_lossy(&compile.stderr)
    );

    let run = Command::new(&exe_path)
        .current_dir(&temp_dir)
        .env(dynamic_library_path_env(), &target_dir)
        .output()
        .unwrap();
    fs::remove_dir_all(temp_dir).unwrap();

    assert!(
        run.status.success(),
        "C++ client exited with {:?}\nstdout:\n{}\nstderr:\n{}",
        run.status.code(),
        String::from_utf8_lossy(&run.stdout),
        String::from_utf8_lossy(&run.stderr)
    );
}

fn build_c_api_library() {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let output = Command::new(cargo)
        .args([
            "build",
            "-p",
            "morfeusz-capi",
            "--lib",
            "--manifest-path",
            "../Cargo.toml",
        ])
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "failed to build C API library:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn target_profile_dir() -> PathBuf {
    let mut path = std::env::current_exe().unwrap();
    path.pop();
    if path.file_name().is_some_and(|name| name == "deps") {
        path.pop();
    }
    path
}

fn shared_library_name() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "libmorfeusz2.dylib"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "libmorfeusz2.so"
    }
    #[cfg(windows)]
    {
        "morfeusz2.dll"
    }
}

fn dynamic_library_path_env() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "DYLD_LIBRARY_PATH"
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        "LD_LIBRARY_PATH"
    }
    #[cfg(windows)]
    {
        "PATH"
    }
}

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../morfeusz-rs/tests/fixtures/binary")
        .join(name)
}

fn unique_temp_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "morfeusz-capi-link-{}-{nanos}-{counter}",
        std::process::id()
    ))
}
