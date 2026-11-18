# Accessibility

Rusted is a server, so most of what players see (menus, text size, controls, narration) comes
from the Minecraft client and its own accessibility settings. Rusted tries not to get in the
way of those.

## What we do
- Forms, chat messages and command output are plain text, so the client's narrator and
  text-to-speech can read them.
- Server messages don't rely on colour alone to say what happened.
- The docs are plain Markdown with headings, readable with a screen reader.

## Supported environments
Any Bedrock client on the supported version (1.26.60) should work: Windows, Xbox,
PlayStation, Switch, iOS and Android.

## Known limitations
- Custom forms from plugins can be less accessible, depending on how they're written.
- Some command errors are short and could be clearer.

## Reporting a barrier
Open an issue and describe what got in your way and on which device. Accessibility issues
are treated as bugs.
