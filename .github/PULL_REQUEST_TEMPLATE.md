## What changes

<!-- What this does and why. Link the issue it closes, if any: "Closes #123". -->

## How it was checked

<!-- Tests added, and how each was proven by breaking the code. For the window: the sizes and systems looked at. -->

## Checklist

- [ ] `scripts/check.sh` passes.
- [ ] `scripts/platforms.sh` passes, or the change cannot touch Windows or Linux.
- [ ] New code has tests, and each new test was seen failing on broken code.
- [ ] A change to the window: looked at at 900 by 580 and at full screen, and the screen pictures written anew if
      it is meant to be seen.
- [ ] New interface text is in `app/ui/say.slint` and in both `app/lang` files.
- [ ] Everything written is in English, outside `app/lang`.
- [ ] Nothing personal reaches the journal.
- [ ] `docs/PROTOCOL.md` records a new API call, and `docs/SECURITY.md` a change to security.
- [ ] No unofficial behaviour towards Proton: no impersonation, no CAPTCHA bypass, no automatic sign-in retries.
