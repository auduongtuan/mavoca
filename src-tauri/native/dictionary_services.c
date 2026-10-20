// Reads entries from the dictionaries installed with macOS (Dictionary.app).
//
// The public DictionaryServices API only returns plain text from the default
// dictionary. The functions below are the private ones Dictionary.app uses:
// they pick a dictionary by name and return each entry as HTML. They are
// resolved with dlsym at runtime, so a macOS release that drops or renames one
// makes the lookup report MAVOCA_DCS_UNAVAILABLE instead of failing to load.

#include <CoreFoundation/CoreFoundation.h>
#include <dlfcn.h>
#include <stdlib.h>
#include <string.h>

#include "dictionary_services.h"

#define DICTIONARY_SERVICES_PATH                                               \
  "/System/Library/Frameworks/CoreServices.framework/Frameworks/"              \
  "DictionaryServices.framework/DictionaryServices"

// The record version that yields the entry's HTML body.
#define RECORD_VERSION_HTML 1
// Records are separated by the ASCII record separator in the returned string.
#define RECORD_SEPARATOR '\x1e'

typedef CFSetRef (*CopyAvailableDictionariesFn)(void);
typedef CFStringRef (*DictionaryGetNameFn)(CFTypeRef dictionary);
typedef CFArrayRef (*CopyRecordsForSearchStringFn)(CFTypeRef dictionary,
                                                   CFStringRef string,
                                                   CFIndex method,
                                                   CFIndex max_results);
typedef CFStringRef (*RecordGetHeadwordFn)(CFTypeRef record);
typedef CFStringRef (*RecordCopyDataFn)(CFTypeRef record, CFIndex version);

static struct {
  int loaded;
  CopyAvailableDictionariesFn copy_available_dictionaries;
  DictionaryGetNameFn dictionary_get_name;
  CopyRecordsForSearchStringFn copy_records_for_search_string;
  RecordGetHeadwordFn record_get_headword;
  RecordCopyDataFn record_copy_data;
} dcs;

static int load_dictionary_services(void) {
  if (dcs.loaded) {
    return dcs.loaded > 0;
  }

  void *handle = dlopen(DICTIONARY_SERVICES_PATH, RTLD_LAZY);
  if (handle) {
    dcs.copy_available_dictionaries =
        (CopyAvailableDictionariesFn)dlsym(handle, "DCSCopyAvailableDictionaries");
    dcs.dictionary_get_name =
        (DictionaryGetNameFn)dlsym(handle, "DCSDictionaryGetName");
    dcs.copy_records_for_search_string =
        (CopyRecordsForSearchStringFn)dlsym(handle, "DCSCopyRecordsForSearchString");
    dcs.record_get_headword =
        (RecordGetHeadwordFn)dlsym(handle, "DCSRecordGetHeadword");
    dcs.record_copy_data = (RecordCopyDataFn)dlsym(handle, "DCSRecordCopyData");
  }

  int complete = handle && dcs.copy_available_dictionaries &&
                 dcs.dictionary_get_name && dcs.copy_records_for_search_string &&
                 dcs.record_get_headword && dcs.record_copy_data;
  dcs.loaded = complete ? 1 : -1;
  return complete;
}

// Copies a CFString into a newly allocated UTF-8 C string.
static char *copy_utf8(CFStringRef string) {
  CFIndex length = CFStringGetLength(string);
  CFIndex size =
      CFStringGetMaximumSizeForEncoding(length, kCFStringEncodingUTF8) + 1;
  char *buffer = malloc((size_t)size);
  if (buffer && !CFStringGetCString(string, buffer, size, kCFStringEncodingUTF8)) {
    free(buffer);
    return NULL;
  }
  return buffer;
}

static CFTypeRef find_dictionary(CFSetRef dictionaries, CFStringRef name) {
  CFIndex count = CFSetGetCount(dictionaries);
  const void **values = malloc(sizeof(void *) * (size_t)count);
  if (!values) {
    return NULL;
  }
  CFSetGetValues(dictionaries, values);

  CFTypeRef match = NULL;
  for (CFIndex index = 0; index < count && !match; index++) {
    CFStringRef candidate = dcs.dictionary_get_name(values[index]);
    if (candidate && CFStringCompare(candidate, name, 0) == kCFCompareEqualTo) {
      match = values[index];
    }
  }
  free(values);
  return match;
}

int mavoca_dcs_lookup(const char *dictionary_name, const char *word,
                      char **html_out) {
  *html_out = NULL;
  if (!load_dictionary_services()) {
    return MAVOCA_DCS_UNAVAILABLE;
  }

  CFStringRef name = CFStringCreateWithCString(NULL, dictionary_name,
                                               kCFStringEncodingUTF8);
  CFStringRef search =
      CFStringCreateWithCString(NULL, word, kCFStringEncodingUTF8);
  CFSetRef dictionaries = dcs.copy_available_dictionaries();
  int status = MAVOCA_DCS_NO_DICTIONARY;
  CFMutableStringRef joined = NULL;

  CFTypeRef dictionary =
      (name && search && dictionaries) ? find_dictionary(dictionaries, name) : NULL;
  CFArrayRef records =
      dictionary ? dcs.copy_records_for_search_string(dictionary, search, 0, 0)
                 : NULL;

  if (dictionary) {
    status = MAVOCA_DCS_NO_ENTRY;
    joined = CFStringCreateMutable(NULL, 0);
    CFIndex count = records ? CFArrayGetCount(records) : 0;
    for (CFIndex index = 0; index < count; index++) {
      CFTypeRef record = CFArrayGetValueAtIndex(records, index);
      // A search also returns phrases and derived words ("run-up" for "run");
      // keep only the entries whose headword is the word itself.
      CFStringRef headword = dcs.record_get_headword(record);
      if (!headword || CFStringCompare(headword, search,
                                       kCFCompareCaseInsensitive) != kCFCompareEqualTo) {
        continue;
      }
      CFStringRef html = dcs.record_copy_data(record, RECORD_VERSION_HTML);
      if (!html) {
        continue;
      }
      if (CFStringGetLength(joined) > 0) {
        UniChar separator = RECORD_SEPARATOR;
        CFStringAppendCharacters(joined, &separator, 1);
      }
      CFStringAppend(joined, html);
      CFRelease(html);
      status = MAVOCA_DCS_OK;
    }
  }

  if (status == MAVOCA_DCS_OK) {
    *html_out = copy_utf8(joined);
    if (!*html_out) {
      status = MAVOCA_DCS_NO_ENTRY;
    }
  }

  if (joined) CFRelease(joined);
  if (records) CFRelease(records);
  if (dictionaries) CFRelease(dictionaries);
  if (search) CFRelease(search);
  if (name) CFRelease(name);
  return status;
}

void mavoca_dcs_free(char *html) { free(html); }
