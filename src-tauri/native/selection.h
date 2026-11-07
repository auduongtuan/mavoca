#ifndef MAVOCA_SELECTION_H
#define MAVOCA_SELECTION_H

// The text selected in the application that is in front of Mavoca, or NULL when
// nothing is selected, the application does not expose its selection, or Mavoca
// has not been granted the Accessibility permission. Release it with
// mavoca_selection_free.
char *mavoca_selected_text(void);

void mavoca_selection_free(char *text);

// Whether Mavoca holds the Accessibility permission, so a failed read can say
// which of the two reasons it failed for: nothing was selected, or the app is
// not allowed to look. Non-zero when it is trusted.
int mavoca_accessibility_trusted(void);

#endif
