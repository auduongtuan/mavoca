// Reads the text selected in whichever application has the keyboard.
//
// The add-word shortcut is handled before the quick-add window takes focus, so
// at that moment the selection still belongs to the other application: the
// system-wide element reports that application, then its focused element, then
// its selected text.
//
// This is the Accessibility API, so it needs the Accessibility permission.
// Without it every call fails with kAXErrorAPIDisabled, and the first attempt
// asks macOS to show its own "grant access" dialog once per launch. A build that
// is re-signed changes the code the permission is remembered against, so a
// reinstalled build asks again.

#include <ApplicationServices/ApplicationServices.h>
#include <stdlib.h>

#include "selection.h"

// A stalled application must not hold the shortcut: AX calls are synchronous
// IPC into it, and the default timeout is measured in seconds.
#define MESSAGING_TIMEOUT_SECONDS 0.4f

static CFTypeRef copy_attribute(AXUIElementRef element, CFStringRef attribute) {
  CFTypeRef value = NULL;
  if (element == NULL) {
    return NULL;
  }
  if (AXUIElementCopyAttributeValue(element, attribute, &value) !=
      kAXErrorSuccess) {
    return NULL;
  }
  return value;
}

// The same, but only when the attribute really is a UI element: an application
// that answers with something else must not be cast and messaged anyway.
static AXUIElementRef copy_element_attribute(AXUIElementRef element,
                                             CFStringRef attribute) {
  CFTypeRef value = copy_attribute(element, attribute);
  if (value == NULL) {
    return NULL;
  }
  if (CFGetTypeID(value) != AXUIElementGetTypeID()) {
    CFRelease(value);
    return NULL;
  }
  return (AXUIElementRef)value;
}

// Asks macOS to point the user at System Settings, at most once per launch.
static void request_accessibility_once(void) {
  static int requested = 0;
  if (requested) {
    return;
  }
  requested = 1;

  CFStringRef keys[] = {kAXTrustedCheckOptionPrompt};
  CFBooleanRef values[] = {kCFBooleanTrue};
  CFDictionaryRef options = CFDictionaryCreate(
      NULL, (const void **)keys, (const void **)values, 1,
      &kCFTypeDictionaryKeyCallBacks, &kCFTypeDictionaryValueCallBacks);
  if (options != NULL) {
    AXIsProcessTrustedWithOptions(options);
    CFRelease(options);
  }
}

char *mavoca_selected_text(void) {
  if (!AXIsProcessTrusted()) {
    request_accessibility_once();
    return NULL;
  }

  AXUIElementRef system = AXUIElementCreateSystemWide();
  if (system == NULL) {
    return NULL;
  }
  AXUIElementSetMessagingTimeout(system, MESSAGING_TIMEOUT_SECONDS);

  char *text_out = NULL;
  AXUIElementRef application =
      copy_element_attribute(system, kAXFocusedApplicationAttribute);
  AXUIElementRef element =
      copy_element_attribute(application, kAXFocusedUIElementAttribute);
  CFTypeRef selection =
      copy_attribute(element, kAXSelectedTextAttribute);

  if (selection != NULL && CFGetTypeID(selection) == CFStringGetTypeID()) {
    CFStringRef text = (CFStringRef)selection;
    CFIndex length = CFStringGetLength(text);
    CFIndex capacity =
        CFStringGetMaximumSizeForEncoding(length, kCFStringEncodingUTF8) + 1;
    if (capacity > 1) {
      text_out = malloc((size_t)capacity);
      if (text_out != NULL &&
          !CFStringGetCString(text, text_out, capacity, kCFStringEncodingUTF8)) {
        free(text_out);
        text_out = NULL;
      }
    }
  }

  if (selection != NULL) {
    CFRelease(selection);
  }
  if (element != NULL) {
    CFRelease(element);
  }
  if (application != NULL) {
    CFRelease(application);
  }
  CFRelease(system);

  return text_out;
}

void mavoca_selection_free(char *text) { free(text); }
