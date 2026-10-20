#ifndef MAVOCA_DICTIONARY_SERVICES_H
#define MAVOCA_DICTIONARY_SERVICES_H

// Status codes returned by mavoca_dcs_lookup; mirrored in macos_dictionary.rs.
#define MAVOCA_DCS_OK 0
#define MAVOCA_DCS_UNAVAILABLE 1
#define MAVOCA_DCS_NO_DICTIONARY 2
#define MAVOCA_DCS_NO_ENTRY 3

// Looks `word` up in the installed dictionary named `dictionary_name`. On
// MAVOCA_DCS_OK, `*html_out` holds every matching entry's HTML, separated by
// the ASCII record separator (0x1E); release it with mavoca_dcs_free.
int mavoca_dcs_lookup(const char *dictionary_name, const char *word,
                      char **html_out);

void mavoca_dcs_free(char *html);

#endif
