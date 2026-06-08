#ifndef MORFEUSZ2_H
#define MORFEUSZ2_H

#include "morfeusz2_c.h"

#include <cstddef>
#include <exception>
#include <list>
#include <map>
#include <set>
#include <stdexcept>
#include <string>
#include <vector>

namespace morfeusz {

struct DLLIMPORT MorphInterpretation;
class DLLIMPORT Morfeusz;
class DLLIMPORT ResultsIterator;
class DLLIMPORT IdResolver;
class DLLIMPORT MorfeuszException;

enum Charset {
    UTF8 = 11,
    ISO8859_2 = 12,
    CP1250 = 13,
    CP852 = 14
};

enum TokenNumbering {
    SEPARATE_NUMBERING = 201,
    CONTINUOUS_NUMBERING = 202
};

enum CaseHandling {
    CONDITIONALLY_CASE_SENSITIVE = 100,
    STRICTLY_CASE_SENSITIVE = 101,
    IGNORE_CASE = 102
};

enum WhitespaceHandling {
    SKIP_WHITESPACES = 301,
    APPEND_WHITESPACES = 302,
    KEEP_WHITESPACES = 303
};

enum MorfeuszUsage {
    ANALYSE_ONLY = 401,
    GENERATE_ONLY = 402,
    BOTH_ANALYSE_AND_GENERATE = 403
};

class DLLIMPORT MorfeuszException : public std::exception {
public:
    MorfeuszException(const std::string& what) : msg(what) {}

    virtual ~MorfeuszException() throw() {}

    virtual const char* what() const throw() {
        return msg.c_str();
    }

private:
    const std::string msg;
};

class DLLIMPORT FileFormatException : public MorfeuszException {
public:
    FileFormatException(const std::string& what) : MorfeuszException(what) {}
};

namespace detail {

inline std::string stringFromC(const char* value) {
    return value == 0 ? std::string() : std::string(value);
}

inline std::string stringFromC(char* value) {
    return stringFromC(static_cast<const char*>(value));
}

inline std::string lastError(::MorfeuszInstance* instance, const std::string& fallback) {
    const char* value = instance == 0 ? morfeusz_last_error() : morfeusz_instance_last_error(instance);
    if (value == 0 || value[0] == '\0') {
        return fallback;
    }
    return std::string(value);
}

inline void splitLabels(const std::string& labels, std::set<std::string>& out) {
    out.clear();
    if (labels.empty() || labels == "_") {
        return;
    }
    std::size_t start = 0;
    while (start <= labels.size()) {
        std::size_t end = labels.find('|', start);
        std::string item = labels.substr(start, end == std::string::npos ? end : end - start);
        if (!item.empty()) {
            out.insert(item);
        }
        if (end == std::string::npos) {
            break;
        }
        start = end + 1;
    }
}

void syncDictionarySearchPaths();

} // namespace detail

class DLLIMPORT IdResolver {
public:
    explicit IdResolver(::MorfeuszInstance* instance = 0) : instance(instance) {}

    virtual const std::string getTagsetId() const {
        return detail::stringFromC(morfeusz_instance_get_tagset_id(instance));
    }

    virtual const std::string& getTag(const int tagId) const {
        std::map<int, std::string>::const_iterator cached = tagCache.find(tagId);
        if (cached != tagCache.end()) {
            return cached->second;
        }
        const char* value = morfeusz_instance_get_tag(instance, tagId);
        if (value == 0) {
            throw std::out_of_range(detail::lastError(instance, "Invalid tagId"));
        }
        std::map<int, std::string>::iterator inserted =
            tagCache.insert(std::make_pair(tagId, std::string(value))).first;
        return inserted->second;
    }

    virtual int getTagId(const std::string& tag) const {
        int id = morfeusz_instance_get_tag_id(instance, tag.c_str());
        if (id < 0) {
            throw MorfeuszException(detail::lastError(instance, "Invalid tag"));
        }
        return id;
    }

    virtual const std::string& getName(const int nameId) const {
        std::map<int, std::string>::const_iterator cached = nameCache.find(nameId);
        if (cached != nameCache.end()) {
            return cached->second;
        }
        const char* value = morfeusz_instance_get_name(instance, nameId);
        if (value == 0) {
            throw std::out_of_range(detail::lastError(instance, "Invalid nameId"));
        }
        std::map<int, std::string>::iterator inserted =
            nameCache.insert(std::make_pair(nameId, std::string(value))).first;
        return inserted->second;
    }

    virtual int getNameId(const std::string& name) const {
        int id = morfeusz_instance_get_name_id(instance, name.c_str());
        if (id < 0) {
            throw MorfeuszException(detail::lastError(instance, "Invalid name"));
        }
        return id;
    }

    virtual const std::string& getLabelsAsString(int labelsId) const {
        std::map<int, std::string>::const_iterator cached = labelsStringCache.find(labelsId);
        if (cached != labelsStringCache.end()) {
            return cached->second;
        }
        const char* value = morfeusz_instance_get_labels_as_string(instance, labelsId);
        if (value == 0) {
            throw std::out_of_range(detail::lastError(instance, "Invalid labelsId"));
        }
        std::map<int, std::string>::iterator inserted =
            labelsStringCache.insert(std::make_pair(labelsId, std::string(value))).first;
        return inserted->second;
    }

    virtual const std::set<std::string>& getLabels(int labelsId) const {
        std::map<int, std::set<std::string> >::const_iterator cached =
            labelsCache.find(labelsId);
        if (cached != labelsCache.end()) {
            return cached->second;
        }
        std::set<std::string> labels;
        detail::splitLabels(getLabelsAsString(labelsId), labels);
        std::map<int, std::set<std::string> >::iterator inserted =
            labelsCache.insert(std::make_pair(labelsId, labels)).first;
        return inserted->second;
    }

    virtual int getLabelsId(const std::string& labelsStr) const {
        int id = morfeusz_instance_get_labels_id(instance, labelsStr.c_str());
        if (id < 0) {
            throw MorfeuszException(detail::lastError(instance, "Invalid labels string"));
        }
        return id;
    }

    virtual size_t getTagsCount() const {
        return morfeusz_instance_get_tags_count(instance);
    }

    virtual size_t getNamesCount() const {
        return morfeusz_instance_get_names_count(instance);
    }

    virtual size_t getLabelsCount() const {
        return morfeusz_instance_get_labels_count(instance);
    }

    virtual ~IdResolver() {}

private:
    friend class Morfeusz;

    void attach(::MorfeuszInstance* newInstance) const {
        instance = newInstance;
        clearCache();
    }

    void clearCache() const {
        tagCache.clear();
        nameCache.clear();
        labelsStringCache.clear();
        labelsCache.clear();
    }

    mutable ::MorfeuszInstance* instance;
    mutable std::map<int, std::string> tagCache;
    mutable std::map<int, std::string> nameCache;
    mutable std::map<int, std::string> labelsStringCache;
    mutable std::map<int, std::set<std::string> > labelsCache;
};

struct DLLIMPORT MorphInterpretation {
    MorphInterpretation()
        : startNode(0), endNode(0), orth(), lemma(), tagId(0), nameId(0), labelsId(0) {}

    static MorphInterpretation createIgn(
            int startNode, int endNode,
            const std::string& orth, const std::string& lemma) {
        MorphInterpretation result;
        result.startNode = startNode;
        result.endNode = endNode;
        result.orth = orth;
        result.lemma = lemma;
        result.tagId = 0;
        result.nameId = 0;
        result.labelsId = 0;
        return result;
    }

    static MorphInterpretation createWhitespace(
            int startNode, int endNode, const std::string& orth) {
        MorphInterpretation result;
        result.startNode = startNode;
        result.endNode = endNode;
        result.orth = orth;
        result.lemma = orth;
        result.tagId = 1;
        result.nameId = 0;
        result.labelsId = 0;
        return result;
    }

    bool isIgn() const {
        return tagId == 0;
    }

    bool isWhitespace() const {
        return tagId == 1;
    }

    const std::string& getTag(const Morfeusz& morfeusz) const;
    const std::string& getName(const Morfeusz& morfeusz) const;
    const std::string& getLabelsAsString(const Morfeusz& morfeusz) const;
    const std::set<std::string>& getLabels(const Morfeusz& morfeusz) const;

    int startNode;
    int endNode;
    std::string orth;
    std::string lemma;
    int tagId;
    int nameId;
    int labelsId;
};

namespace detail {

inline MorphInterpretation fromC(const ::MorfeuszInterp& value) {
    MorphInterpretation result;
    result.startNode = value.startNode;
    result.endNode = value.endNode;
    result.orth = stringFromC(value.orth);
    result.lemma = stringFromC(value.lemma);
    result.tagId = value.tagId;
    result.nameId = value.nameId;
    result.labelsId = value.labelsId;
    return result;
}

inline void copyOwnedInterps(::MorfeuszOwnedInterps* owned,
                             std::vector<MorphInterpretation>& result) {
    if (owned == 0) {
        return;
    }
    const size_t len = morfeusz_interps_len(owned);
    const ::MorfeuszInterp* data = morfeusz_interps_data(owned);
    result.reserve(result.size() + len);
    for (size_t i = 0; i < len; ++i) {
        result.push_back(fromC(data[i]));
    }
    morfeusz_destroy_interps(owned);
}

} // namespace detail

class DLLIMPORT ResultsIterator {
public:
    ResultsIterator() : items(), index(0) {}

    explicit ResultsIterator(const std::vector<MorphInterpretation>& items)
        : items(items), index(0) {}

    virtual bool hasNext() {
        return index < items.size();
    }

    virtual const MorphInterpretation& peek() {
        if (!hasNext()) {
            throw std::out_of_range("No more interpretations available to ResultsIterator");
        }
        return items[index];
    }

    virtual MorphInterpretation next() {
        MorphInterpretation result = peek();
        ++index;
        return result;
    }

    virtual ~ResultsIterator() {}

private:
    std::vector<MorphInterpretation> items;
    size_t index;
};

class DLLIMPORT Morfeusz {
public:
    static std::string getVersion() {
        return detail::stringFromC(morfeusz_about());
    }

    static std::string getDefaultDictName() {
        return detail::stringFromC(morfeusz_get_default_dict_name());
    }

    static std::string getCopyright() {
        return detail::stringFromC(morfeusz_get_copyright());
    }

    static Morfeusz* createInstance(MorfeuszUsage usage = BOTH_ANALYSE_AND_GENERATE) {
        detail::syncDictionarySearchPaths();
        ::MorfeuszInstance* instance = morfeusz_create_instance(static_cast<int>(usage));
        if (instance == 0) {
            throw MorfeuszException(detail::lastError(0, "Cannot create Morfeusz instance"));
        }
        return new Morfeusz(instance);
    }

    static Morfeusz* createInstance(
            const std::string& dictName,
            MorfeuszUsage usage = BOTH_ANALYSE_AND_GENERATE) {
        detail::syncDictionarySearchPaths();
        ::MorfeuszInstance* instance =
            morfeusz_create_instance_named(dictName.c_str(), static_cast<int>(usage));
        if (instance == 0) {
            throw MorfeuszException(detail::lastError(0, "Cannot create Morfeusz instance"));
        }
        return new Morfeusz(instance);
    }

    virtual std::string getDictID() const {
        return detail::stringFromC(morfeusz_instance_get_dict_id(instance));
    }

    virtual std::string getDictCopyright() const {
        return detail::stringFromC(morfeusz_instance_get_dict_copyright(instance));
    }

    virtual Morfeusz* clone() const {
        ::MorfeuszInstance* cloned = morfeusz_clone_instance(instance);
        if (cloned == 0) {
            throw MorfeuszException("Cannot clone Morfeusz instance");
        }
        return new Morfeusz(cloned);
    }

    virtual ~Morfeusz() {
        morfeusz_destroy_instance(instance);
        instance = 0;
        resolver.attach(0);
    }

    virtual ResultsIterator* analyse(const std::string& text) const {
        std::vector<MorphInterpretation> result;
        analyse(text, result);
        return new ResultsIterator(result);
    }

    virtual ResultsIterator* analyse(const char* text) const {
        return analyse(std::string(text == 0 ? "" : text));
    }

    virtual void analyse(const std::string& text,
                         std::vector<MorphInterpretation>& result) const {
        ::MorfeuszOwnedInterps* owned = morfeusz_instance_analyse(instance, text.c_str());
        if (owned == 0) {
            throw MorfeuszException(detail::lastError(instance, "Cannot analyse"));
        }
        detail::copyOwnedInterps(owned, result);
    }

    virtual void generate(const std::string& lemma,
                          std::vector<MorphInterpretation>& result) const {
        ::MorfeuszOwnedInterps* owned = morfeusz_instance_generate(instance, lemma.c_str());
        if (owned == 0) {
            throw MorfeuszException(detail::lastError(instance, "Cannot generate"));
        }
        detail::copyOwnedInterps(owned, result);
    }

    virtual void generate(const std::string& lemma, int tagId,
                          std::vector<MorphInterpretation>& result) const {
        ::MorfeuszOwnedInterps* owned =
            morfeusz_instance_generate_by_tag_id(instance, lemma.c_str(), tagId);
        if (owned == 0) {
            throw MorfeuszException(detail::lastError(instance, "Cannot generate"));
        }
        detail::copyOwnedInterps(owned, result);
    }

    virtual void setCharset(Charset encoding) {
        if (!morfeusz_instance_set_charset(instance, static_cast<int>(encoding))) {
            throw std::invalid_argument(detail::lastError(instance, "Invalid charset option"));
        }
        resolver.clearCache();
    }

    virtual Charset getCharset() const {
        return static_cast<Charset>(morfeusz_instance_get_charset(instance));
    }

    virtual void setAggl(const std::string& aggl) {
        if (!morfeusz_instance_set_aggl(instance, aggl.c_str())) {
            throw MorfeuszException(detail::lastError(instance, "Invalid aggl option"));
        }
    }

    virtual std::string getAggl() const {
        return detail::stringFromC(morfeusz_instance_get_aggl(instance));
    }

    virtual void setPraet(const std::string& praet) {
        if (!morfeusz_instance_set_praet(instance, praet.c_str())) {
            throw MorfeuszException(detail::lastError(instance, "Invalid praet option"));
        }
    }

    virtual std::string getPraet() const {
        return detail::stringFromC(morfeusz_instance_get_praet(instance));
    }

    virtual void setCaseHandling(CaseHandling caseHandling) {
        if (!morfeusz_instance_set_case_handling(instance, static_cast<int>(caseHandling))) {
            throw std::invalid_argument(detail::lastError(instance, "Invalid case handling option"));
        }
    }

    virtual CaseHandling getCaseHandling() const {
        return static_cast<CaseHandling>(morfeusz_instance_get_case_handling(instance));
    }

    virtual void setTokenNumbering(TokenNumbering numbering) {
        if (!morfeusz_instance_set_token_numbering(instance, static_cast<int>(numbering))) {
            throw std::invalid_argument(detail::lastError(instance, "Invalid token numbering option"));
        }
    }

    virtual TokenNumbering getTokenNumbering() const {
        return static_cast<TokenNumbering>(morfeusz_instance_get_token_numbering(instance));
    }

    virtual void setWhitespaceHandling(WhitespaceHandling whitespaceHandling) {
        if (!morfeusz_instance_set_whitespace_handling(
                instance, static_cast<int>(whitespaceHandling))) {
            throw std::invalid_argument(
                detail::lastError(instance, "Invalid whitespace handling option"));
        }
    }

    virtual WhitespaceHandling getWhitespaceHandling() const {
        return static_cast<WhitespaceHandling>(
            morfeusz_instance_get_whitespace_handling(instance));
    }

    virtual void setDebug(bool debug) {
        morfeusz_instance_set_debug(instance, debug ? 1 : 0);
    }

    virtual const IdResolver& getIdResolver() const {
        return resolver;
    }

    virtual void setDictionary(const std::string& dictName) {
        detail::syncDictionarySearchPaths();
        if (!morfeusz_instance_set_dictionary(instance, dictName.c_str())) {
            throw MorfeuszException(detail::lastError(instance, "Cannot set dictionary"));
        }
        resolver.clearCache();
    }

    static std::list<std::string> dictionarySearchPaths;

    virtual const std::set<std::string>& getAvailableAgglOptions() const {
        availableAgglOptions.clear();
        size_t count = morfeusz_instance_available_aggl_count(instance);
        for (size_t i = 0; i < count; ++i) {
            availableAgglOptions.insert(
                detail::stringFromC(morfeusz_instance_available_aggl_item(instance, i)));
        }
        return availableAgglOptions;
    }

    virtual const std::set<std::string>& getAvailablePraetOptions() const {
        availablePraetOptions.clear();
        size_t count = morfeusz_instance_available_praet_count(instance);
        for (size_t i = 0; i < count; ++i) {
            availablePraetOptions.insert(
                detail::stringFromC(morfeusz_instance_available_praet_item(instance, i)));
        }
        return availablePraetOptions;
    }

protected:
    Morfeusz() : instance(0), resolver(0), availableAgglOptions(), availablePraetOptions() {}

    virtual ResultsIterator* analyseWithCopy(const char* text) const {
        return analyse(std::string(text == 0 ? "" : text));
    }

private:
    explicit Morfeusz(::MorfeuszInstance* instance)
        : instance(instance), resolver(instance), availableAgglOptions(), availablePraetOptions() {}

    Morfeusz(const Morfeusz&);
    Morfeusz& operator=(const Morfeusz&);

    mutable ::MorfeuszInstance* instance;
    mutable IdResolver resolver;
    mutable std::set<std::string> availableAgglOptions;
    mutable std::set<std::string> availablePraetOptions;
};

#if defined(_MSC_VER)
__declspec(selectany) std::list<std::string> Morfeusz::dictionarySearchPaths(1, ".");
#elif defined(__GNUC__) || defined(__clang__)
__attribute__((weak)) std::list<std::string> Morfeusz::dictionarySearchPaths(1, ".");
#else
std::list<std::string> Morfeusz::dictionarySearchPaths(1, ".");
#endif

namespace detail {

inline void syncDictionarySearchPaths() {
    morfeusz_dictionary_search_paths_clear();
    for (std::list<std::string>::const_iterator it = Morfeusz::dictionarySearchPaths.begin();
         it != Morfeusz::dictionarySearchPaths.end(); ++it) {
        morfeusz_dictionary_search_paths_push(it->c_str());
    }
}

} // namespace detail

inline const std::string& MorphInterpretation::getTag(const Morfeusz& morfeusz) const {
    return morfeusz.getIdResolver().getTag(this->tagId);
}

inline const std::string& MorphInterpretation::getName(const Morfeusz& morfeusz) const {
    return morfeusz.getIdResolver().getName(this->nameId);
}

inline const std::string& MorphInterpretation::getLabelsAsString(
        const Morfeusz& morfeusz) const {
    return morfeusz.getIdResolver().getLabelsAsString(this->labelsId);
}

inline const std::set<std::string>& MorphInterpretation::getLabels(
        const Morfeusz& morfeusz) const {
    return morfeusz.getIdResolver().getLabels(this->labelsId);
}

} // namespace morfeusz

#endif
