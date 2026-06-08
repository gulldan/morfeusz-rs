#ifndef MORFEUSZ2_C_H
#define MORFEUSZ2_C_H

#ifndef __MORFEUSZ_H__
#define __MORFEUSZ_H__
#endif

#include <stddef.h>

#ifndef __WIN32
#define DLLIMPORT
#else
#if BUILDING_MORFEUSZ
#define DLLIMPORT __declspec(dllexport)
#else
#define DLLIMPORT __declspec(dllimport)
#endif
#endif

#ifdef __cplusplus
extern "C" {
#endif

DLLIMPORT char *morfeusz_about(void);
DLLIMPORT char *morfeusz_get_default_dict_name(void);
DLLIMPORT char *morfeusz_get_copyright(void);
DLLIMPORT const char *morfeusz_last_error(void);

struct _InterpMorf {
  int p;
  int k;
  char *forma;
  char *haslo;
  char *interp;
};
typedef struct _InterpMorf InterpMorf;

DLLIMPORT InterpMorf *morfeusz_analyse(char *tekst);

DLLIMPORT int morfeusz_set_option(int option, int value);

DLLIMPORT void morfeusz_dictionary_search_paths_clear(void);
DLLIMPORT int morfeusz_dictionary_search_paths_push(const char *path);
DLLIMPORT size_t morfeusz_dictionary_search_paths_count(void);
DLLIMPORT const char *morfeusz_dictionary_search_paths_item(size_t index);

struct _MorfeuszInstance;
typedef struct _MorfeuszInstance MorfeuszInstance;

struct _MorfeuszOwnedInterps;
typedef struct _MorfeuszOwnedInterps MorfeuszOwnedInterps;

struct _MorfeuszInterp {
  int startNode;
  int endNode;
  const char *orth;
  const char *lemma;
  int tagId;
  int nameId;
  int labelsId;
};
typedef struct _MorfeuszInterp MorfeuszInterp;

DLLIMPORT MorfeuszInstance *morfeusz_create_instance(int usage);
DLLIMPORT MorfeuszInstance *morfeusz_create_instance_named(const char *dict_name,
                                                           int usage);
DLLIMPORT MorfeuszInstance *morfeusz_clone_instance(
    const MorfeuszInstance *instance);
DLLIMPORT void morfeusz_destroy_instance(MorfeuszInstance *instance);
DLLIMPORT const char *morfeusz_instance_last_error(
    const MorfeuszInstance *instance);

DLLIMPORT const char *morfeusz_instance_get_dict_id(
    MorfeuszInstance *instance);
DLLIMPORT const char *morfeusz_instance_get_dict_copyright(
    MorfeuszInstance *instance);

DLLIMPORT MorfeuszOwnedInterps *morfeusz_instance_analyse(
    MorfeuszInstance *instance, const char *text);
DLLIMPORT MorfeuszOwnedInterps *morfeusz_instance_generate(
    MorfeuszInstance *instance, const char *lemma);
DLLIMPORT MorfeuszOwnedInterps *morfeusz_instance_generate_by_tag_id(
    MorfeuszInstance *instance, const char *lemma, int tag_id);
DLLIMPORT size_t morfeusz_interps_len(const MorfeuszOwnedInterps *results);
DLLIMPORT const MorfeuszInterp *morfeusz_interps_data(
    const MorfeuszOwnedInterps *results);
DLLIMPORT void morfeusz_destroy_interps(MorfeuszOwnedInterps *results);

DLLIMPORT int morfeusz_instance_set_charset(MorfeuszInstance *instance,
                                            int value);
DLLIMPORT int morfeusz_instance_get_charset(const MorfeuszInstance *instance);
DLLIMPORT int morfeusz_instance_set_aggl(MorfeuszInstance *instance,
                                         const char *value);
DLLIMPORT const char *morfeusz_instance_get_aggl(MorfeuszInstance *instance);
DLLIMPORT int morfeusz_instance_set_praet(MorfeuszInstance *instance,
                                          const char *value);
DLLIMPORT const char *morfeusz_instance_get_praet(MorfeuszInstance *instance);
DLLIMPORT int morfeusz_instance_set_case_handling(MorfeuszInstance *instance,
                                                  int value);
DLLIMPORT int morfeusz_instance_get_case_handling(
    const MorfeuszInstance *instance);
DLLIMPORT int morfeusz_instance_set_token_numbering(MorfeuszInstance *instance,
                                                    int value);
DLLIMPORT int morfeusz_instance_get_token_numbering(
    const MorfeuszInstance *instance);
DLLIMPORT int morfeusz_instance_set_whitespace_handling(
    MorfeuszInstance *instance, int value);
DLLIMPORT int morfeusz_instance_get_whitespace_handling(
    const MorfeuszInstance *instance);
DLLIMPORT int morfeusz_instance_set_debug(MorfeuszInstance *instance,
                                          int debug);
DLLIMPORT int morfeusz_instance_set_dictionary(MorfeuszInstance *instance,
                                               const char *dict_name);

DLLIMPORT size_t morfeusz_instance_available_aggl_count(
    const MorfeuszInstance *instance);
DLLIMPORT const char *morfeusz_instance_available_aggl_item(
    MorfeuszInstance *instance, size_t index);
DLLIMPORT size_t morfeusz_instance_available_praet_count(
    const MorfeuszInstance *instance);
DLLIMPORT const char *morfeusz_instance_available_praet_item(
    MorfeuszInstance *instance, size_t index);

DLLIMPORT const char *morfeusz_instance_get_tagset_id(
    MorfeuszInstance *instance);
DLLIMPORT const char *morfeusz_instance_get_tag(MorfeuszInstance *instance,
                                                int tag_id);
DLLIMPORT int morfeusz_instance_get_tag_id(MorfeuszInstance *instance,
                                           const char *tag);
DLLIMPORT const char *morfeusz_instance_get_name(MorfeuszInstance *instance,
                                                 int name_id);
DLLIMPORT int morfeusz_instance_get_name_id(MorfeuszInstance *instance,
                                            const char *name);
DLLIMPORT const char *morfeusz_instance_get_labels_as_string(
    MorfeuszInstance *instance, int labels_id);
DLLIMPORT int morfeusz_instance_get_labels_id(MorfeuszInstance *instance,
                                              const char *labels);
DLLIMPORT size_t morfeusz_instance_get_tags_count(
    const MorfeuszInstance *instance);
DLLIMPORT size_t morfeusz_instance_get_names_count(
    const MorfeuszInstance *instance);
DLLIMPORT size_t morfeusz_instance_get_labels_count(
    const MorfeuszInstance *instance);

#define MORFOPT_ENCODING 1

#define MORFEUSZ_UTF_8 8
#define MORFEUSZ_ISO8859_2 88592
#define MORFEUSZ_CP1250 1250
#define MORFEUSZ_CP852 852

#define MORFOPT_WHITESPACE 2

#define MORFEUSZ_SKIP_WHITESPACE 0
#define MORFEUSZ_KEEP_WHITESPACE 2
#define MORFEUSZ_APPEND_WHITESPACE 4

#define MORFOPT_CASE 3

#define MORFEUSZ_WEAK_CASE 301
#define MORFEUSZ_STRICT_CASE 302
#define MORFEUSZ_IGNORE_CASE 303

#define MORFOPT_TOKEN_NUMBERING 4

#define MORFEUSZ_SEPARATE_TOKEN_NUMBERING 401
#define MORFEUSZ_CONTINUOUS_TOKEN_NUMBERING 402

#ifdef __cplusplus
}
#endif

#endif
