# Wordbar on Waybar

`wordbarctl` already speaks the full protocol, so Waybar needs no second
application — just a module that polls it.

Add to `~/.config/waybar/config`:

```jsonc
"custom/wordbar": {
    "exec": "wordbarctl current --waybar",
    "return-type": "json",
    "interval": 60,
    "on-click": "wordbarctl next",
    "on-click-right": "wordbarctl know",
    "on-click-middle": "wordbarctl again",
    "tooltip": true
}
```

and place `"custom/wordbar"` in `modules-center` (or wherever you like).

## Why poll `current` and not `next`

```text
current    read-only     safe to poll
next       rotates       user-initiated only
```

`current` returns the same word every time until something actually changes it,
so a 60-second poll does not turn into a 60-second rotation. Polling `next`
would rotate the word on every tick, and would also fight with the GNOME
extension if both are running.

Rotation on Waybar is therefore manual (click) rather than timed. If you want
timed rotation without GNOME Shell, run `wordbarctl next` from a systemd timer
or a cron job at your preferred interval.

## Styling

The module sets a `class` you can style:

| class        | meaning                                    |
|--------------|--------------------------------------------|
| `learning`   | a word with a definition                   |
| `incomplete` | added, but enrichment has not run yet      |
| `empty`      | the vocabulary has no active words         |

```css
#custom-wordbar.incomplete { opacity: 0.7; }
#custom-wordbar.empty      { opacity: 0.5; font-style: italic; }
```

## Notes

* The first poll activates the service through D-Bus, exactly as the GNOME
  extension does. Nothing needs to be started in advance.
* `current --waybar` always exits 0 and always prints one valid JSON object,
  including when the vocabulary is empty or the service is unavailable — Waybar
  breaks on anything else.
* Quick capture is a GNOME Shell feature. On Waybar, capture with
  `wordbarctl add <word>`, or bind it in your compositor:

  ```
  bindsym $mod+Shift+v exec wordbarctl add "$(wl-paste --primary)"
  ```
