# UI guidelines

These guidelines are for everyone who changes one of kjk's screens. They say what a screen must keep so that it still feels like KOTOR, and what it must reach to be readable and comfortable by the standards of 2026. The owner's direction is "KOTOR, readable and comfortable, but in 2026". Any screen may be redesigned, and its UX may change, but the result must still feel like KOTOR.

The page has three parts. The principles explain how to change a screen without losing KOTOR's feel. The section on sizes and distances explains which screens and viewing distances the rules are checked at. The rules are numbered, and each one names its source and how it is checked. The lint (`kjk gui lint`) reports its failures under these numbers, and reviewers score screens against the same numbers.

The research behind this page is in four notes on the branch [`research/ui-reference`](https://github.com/Superd22/kjk/tree/research/ui-reference/docs/research). The main one is [Game UI standards for 2026](https://github.com/Superd22/kjk/blob/research/ui-reference/docs/research/2026-11-25-game-ui-standards-2026.md). The plan of the whole work is the roadmap card https://github.com/kjk-engine/roadmap/issues/19.

docs/UI.md describes how the screens are built. docs/PLAYER-TEXT-GUIDELINES.md describes how to write the words on them.

## Principles

These principles come from how remasters of classic games changed their UI without losing the original's feel. Each one names the games it comes from.

1. **Keep what makes a screen recognisable.** A screen keeps the shape of its layout, which is where each region sits and how large it is next to the others. It also keeps KOTOR's frame art, its colours, its fonts and its sounds. Diablo II Resurrected, Age of Empires III Definitive Edition and the Baldur's Gate Enhanced Edition all kept these.
2. **Change how the player gets around, not how the screen looks.** Most changes should be to navigation, controller focus, button prompts, text size, sorting and filtering. That is where Titan Quest, Diablo II Resurrected and Mass Effect Legendary Edition made their changes.
3. **Every new helper can be switched off.** A player who wants the original game must be able to get it back. Diablo II Resurrected's miss indicator is on by default, and it can be turned off.
4. **When players will disagree, ship both.** If a change touches something players love, kjk offers the original form and the new one, with a setting to switch between them. Age of Empires III Definitive Edition ships three HUD layouts, one of them the original. Final Fantasy Pixel Remaster lets the player swap its fonts.
5. **Try the modern form, look at it, and be ready to go back.** According to a developer interview that the research could only read in summary, the Diablo II Resurrected team turned the inventory grid into a list for controllers. They found the list usable, and still went back to the grid, because without it the game no longer felt like Diablo II.
6. **Borrow from KOTOR 2 before inventing.** When KOTOR 2 already solved a problem, its solution comes first. kjk already draws a KOTOR 1 campaign with KOTOR 2's layouts. Mass Effect Legendary Edition moved the first game's UI towards the third's in the same way.
7. **Every change gives its reason.** A change names the rule on this page that it fixes. A change that fixes no rule needs the owner's approval. ROADMAP.md asks the same of every divergence from the games. Each one is named and approved.
8. **Every change passes one question.** A reviewer must be able to answer yes to "Does this still feel like KOTOR?". The Baldur's Gate team asked the same question about their expansion.

## Sizes and distances

Every screen is checked at six sizes. Each size has a scale factor, because kjk draws a layout's pixel as one window point, and the scale factor changes the size of everything on screen.

| Size | Resolution | Scale factor | What it stands for |
|---|---|---|---|
| Steam Deck | 1280 × 800 | 100% | The Steam Deck, held in the hands |
| 1080p | 1920 × 1080 | 100% | A desktop monitor, or a 1080p TV |
| 4K TV | 3840 × 2160 | 100% | A 4K TV |
| 4K monitor | 3840 × 2160 | 200% | A 4K desktop monitor |
| 4:3 | 1440 × 1080 | 100% | An old 4:3 monitor |
| Ultrawide | 3440 × 1440 | 100% | A 21:9 monitor |

The 4K monitor draws the same picture as the 4K TV. kjk scales its screens to the window, so the desktop's scaling does not change their size. The 4K monitor stays in the list to show that kjk does not follow the desktop's scaling. If kjk ever followed it, the two pictures would differ.

A screen can also be seen from three distances. The Steam Deck is held at about 30 cm, which is the distance Valve's rules assume. A desktop monitor is seen from about 60 cm. A TV is seen from between 2.5 and 3 m, which is the "10-foot" view that Microsoft's TV numbers assume. The rules give the desk figure and the TV figure separately where they differ.

Whether the TV rules apply always, only in a TV mode, or only with a large text option is not decided yet. The owner will decide after seeing them on real screens. Until then, the lint reports desk failures and TV failures separately, and a screen only needs to pass the desk rules and the Steam Deck rules.

### How text height is measured

The text rules use the **body height** of a text. It is the number of screen pixels from the top of the tallest letter, such as the stem of an "h", to the bottom of the lowest tail, such as the tail of a "g". A pixel only counts if its contrast with the background is above 2 to 1, so a dark outline does not add to the height. This is how the Xbox Accessibility Guidelines measure text. It is measured on the picture kjk draws, after kjk's own scaling of the fonts.

The minimum figures scale with the height of the screen. The table gives them for each size.

| Size | Steam Deck, at 30 cm | Desk, at 60 cm | TV, at 2.5 to 3 m |
|---|---|---|---|
| Steam Deck (800 pixels high) | 9 px at least, 12 px recommended | not used | not used |
| 1080p, and 4:3 (1080 pixels high) | not used | 18 px | 26 px |
| Ultrawide (1440 pixels high) | not used | 24 px | 35 px |
| 4K, at either scale factor (2160 pixels high) | not used | 36 px | 52 px |

The Steam Deck figures are Valve's. Valve says that "the smallest on-screen font character should never fall below 9 pixels in height at 1280x800", and that it recommends "aiming for 12px whenever possible". Valve does not say how the height is measured, so kjk measures it as body height, like the other figures.

## The rules

Each rule has an ID, a level, a check and a source.

- The **level** is "must" or "should". A "must" rule comes from a guideline with a number or from a platform's pass or fail rule. A "should" rule comes from a weaker source, or is a recommendation.
- The **check** says who decides. "Auto" means the lint decides from the screen's control tree. "Auto, from the picture" means the lint also reads the rendered picture. "Person" means someone must look at the screen or use it. "Judge" means a reviewer judges a picture against the rule's question. A judge's verdict is advice and not a measure.
- **XAG** means the Xbox Accessibility Guidelines.

A rule that does not fit kjk was left out. The list of those is at the end of the page.

### Text (T)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| T-1 | Every text has at least the body height the table above gives for the size and the distance. | must | Auto | [XAG 101](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/101) |
| T-2 | On the Steam Deck, no text is under 9 px high, and body text is at least 12 px high. | must for 9 px, should for 12 px | Auto | [Valve, Steam Deck compatibility review](https://partner.steamgames.com/doc/steamdeck/compat) |
| T-3 | The player can scale the text up to 200%. Nothing is lost or cut off at 200%, and nothing needs scrolling in two directions. | must | Auto for cut-off text. Person for lost meaning. | XAG 101 |
| T-4 | A heading stays visibly larger than the body text at every text scale. | should | Auto | XAG 101 |
| T-5 | Icons and button prompts are at least as large as the minimum text size, and they scale with the text. | must | Auto | XAG 101 |
| T-6 | A block of text longer than two lines has at most 80 characters on a line, not counting spaces. | should | Auto | XAG 101 |
| T-7 | A block of text longer than two lines has a line spacing of at least 1.5 times the font size, or the player can change the spacing. | should | Auto | XAG 101 |
| T-8 | The player can choose a plain font next to KOTOR's own fonts, and every text can be shown in mixed case and not only in capitals. | should | Auto for the setting. Person for the case. | XAG 101 |
| T-9 | Text that runs over several lines is left aligned. Short labels of one or two words may be centred. | should | Auto | XAG 101 |
| T-10 | Interface text can be read by a 12 year old. Story text, such as conversations and the journal's entries, is not held to this rule. | should | Judge | [XAG 114](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/114) |
| T-11 | A picture that carries information has no text in it, apart from logos. | should | Person | [XAG 102](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/102) |
| T-12 | Subtitles are in mixed case. A subtitle line has at most 40 characters, and at most two lines are on screen. Subtitles scale to 200%, can have a background whose opacity the player sets from 0% to 100%, and name the speaker when the speaker changes. | must | Auto for the size, the length and the setting. Person for the speaker. | [XAG 104](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/104) |

### Contrast and colour (C)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| C-1 | Normal text has a contrast of at least 4.5 to 1 against the darkest or lightest part of the background behind it, whichever is closer to the text's colour. | must | Auto, from the picture | [XAG 102](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/102) |
| C-2 | Large text has a contrast of at least 3 to 1. Large text means at least twice the minimum body height of T-1. Text on a disabled or locked control also needs at least 3 to 1. | must | Auto, from the picture | XAG 102 |
| C-3 | Text typed into an edit box, and the hint text shown before the player types, has a contrast of at least 4.5 to 1. | must | Auto, from the picture | XAG 102 |
| C-4 | The parts that show what a control is, such as its border, its icon or a slider's thumb, have a contrast of at least 3 to 1 against what is next to them. | must | Auto, from the picture | [WCAG 2.2, 1.4.11](https://www.w3.org/TR/WCAG22/) |
| C-5 | A high contrast mode exists. In that mode, every element has a contrast of at least 7 to 1. | should | Auto, from the picture | XAG 102 |
| C-6 | Colour is never the only way information is shown. A selected, disabled, locked, good or bad state also has a shape, an icon or a word. | must | Judge, helped by a colour blindness simulation | XAG 102 |
| C-7 | The player can put a solid background behind on-screen text and set its opacity. | should | Auto for the setting | XAG 102 |

### Layout and size (L)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| L-1 | At TV distance, no text or control lies within 5% of an edge of the screen. That is 96 px at the sides and 54 px at the top and bottom at 1080p. Panel backgrounds may reach the edge. The Steam Deck and monitors do not need this. | must at TV distance | Auto | [Microsoft, Designing for Xbox and TV](https://learn.microsoft.com/en-us/previous-versions/windows/uwp/xbox-apps/designing-for-tv) |
| L-2 | At TV distance, every control the player can activate is at least 64 px high at 1080p. | must at TV distance | Auto | Microsoft, Designing for Xbox and TV |
| L-3 | With a mouse, every control the player can click is at least 24 × 24 px, or has enough empty space around it that a 24 px circle around it touches no other control. | must | Auto | [WCAG 2.2, 2.5.8](https://www.w3.org/WAI/WCAG22/Understanding/target-size-minimum.html) |
| L-4 | No two controls overlap unless the layout means them to, and no list, grid or text runs past its panel. | must | Auto | [Apple, Designing for games](https://developer.apple.com/design/human-interface-guidelines/designing-for-games) |
| L-5 | Crossing the screen from one side to the other takes at most six presses of a direction. | should | Auto | Microsoft, Designing for Xbox and TV |
| L-6 | At each of the six sizes, nothing is cut off, overlapped or out of reach. | must | Auto | Apple, Designing for games |
| L-7 | The same kind of element sits in the same place on every screen. Examples are the screen's title, the bar of button prompts and the way back. | should | Auto for the prompt bar. Person for the rest. | [XAG 112](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/112) |
| L-8 | A label sits next to the control it describes, or inside the same focus frame. | should | Auto | XAG 114 |
| L-9 | A long block of text has headings. | should | Judge | XAG 114 |

### Focus and navigation (N)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| N-1 | Every control the player can activate can be reached with the d-pad or the arrow keys alone, starting from the control that has focus when the screen opens. | must | Auto | [XAG 112](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/112) |
| N-2 | Focus never gets stuck. From every control, focus can leave with the same kind of input that brought it there. | must | Auto | XAG 112 |
| N-3 | A direction moves focus to the control that lies in that direction on screen. A link that goes elsewhere is reported, so that a person can confirm it is meant. | should | Auto, with a person for the reported links | XAG 112 |
| N-4 | If Right leads from control A to control B, then Left leads from B back to A. The same holds for Up and Down. A screen may write down its exceptions. | should | Auto | XAG 112 |
| N-5 | A list of one column or one row wraps from its last item to its first. A grid does not wrap. | should | Auto | XAG 112 |
| N-6 | The control with focus is clearly different from every other control. The difference has a contrast of at least 3 to 1, over at least a 2 px border around the control. | must | Auto, from the picture | [WCAG 2.2, 2.4.13](https://www.w3.org/TR/WCAG22/) |
| N-7 | The control with focus is never hidden behind another element, and a list scrolls to keep it in view. | must | Auto | [WCAG 2.2, 2.4.11](https://www.w3.org/TR/WCAG22/) |
| N-8 | After the window changes size or the text scale changes, the focus order still follows the layout. | must | Auto | XAG 112 |
| N-9 | Every screen has a way back, with Back on a controller and Escape on a keyboard. It always leads to the same kind of place. | must | Auto | XAG 112 |
| N-10 | On every screen with tabs, the left and right shoulder buttons change the tab. | must | Auto | [Apple, Game controls](https://developer.apple.com/design/human-interface-guidelines/game-controls) and XAG 112 |
| N-11 | A row of a list holds no second control that needs its own focus step. Such actions move to a menu that the row opens. | should | Auto | Microsoft, Designing for Xbox and TV |
| N-12 | A long list, such as the inventory, a store or the journal, has a second way to find an entry, such as a filter, a sort or a search. | must | Person | XAG 112 |
| N-13 | The area map and the galaxy map have a list of places as well, so the player does not have to move a cursor over the map. | should | Person | XAG 112 |
| N-14 | Every screen can be reached from the main menu or the in-game menu, and no screen opens without the player asking for it, unless a notice says why. | must | Person | XAG 112 and XAG 114 |

### Input and button prompts (I)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| I-1 | Every screen can be used fully with a controller alone. | must | Auto for reaching every control. Person for the rest, including typing text. | [XAG 107](https://learn.microsoft.com/en-us/gaming/accessibility/xbox-accessibility-guidelines/107) and [Valve, Steam Deck compatibility review](https://partner.steamgames.com/doc/steamdeck/compat) |
| I-2 | Every screen can be used fully with a keyboard alone. | must | Auto for reaching every control. Person for the rest. | XAG 107 |
| I-3 | Every screen can be used fully with a mouse alone, and the way back is visible. | must | Person | Apple, Game controls |
| I-4 | Every button prompt shows the buttons of the device the player used last. After a controller press, no keyboard or mouse prompt is shown. | must | Auto | Valve, Steam Deck compatibility review |
| I-5 | Button prompts match the controller type, such as Xbox, PlayStation or Steam Deck, or the player can choose the type. | should | Auto for the setting. Person for the art. | Apple, Game controls |
| I-6 | After the player changes a binding, every prompt, hint and tutorial shows the new binding. | must | Auto | XAG 107 |
| I-7 | The bar of button prompts lists the actions that exist on this screen with this device, and only those. | must | Auto | XAG 112 |
| I-8 | When a screen needs text, such as a character's name or a save's name, a controller player can type it. kjk opens an on-screen keyboard, or has its own way of typing with a controller. | must | Person | Valve, Steam Deck compatibility review |
| I-9 | A click acts when the mouse button is released, not when it is pressed. Moving the pointer off the control before releasing cancels the click. | must | Auto for the event. Person for the feel. | XAG 107 |
| I-10 | No action needs two buttons at once, a drag, or a hold of more than 3 seconds, unless a simpler way to do it also exists. | must | Auto | XAG 107 |
| I-11 | Every action done by holding a button can also be done by tapping it, with a setting. | should | Auto for the setting | XAG 107 and [EA, Dragon Age: The Veilguard](https://www.ea.com/able/resources/dragon-age-the-veilguard) |
| I-12 | Every control can be rebound in the game, on the keyboard and on the controller, including Escape and the stick clicks. | must | Auto | XAG 107 |
| I-13 | The sticks' and triggers' dead zones and sensitivity can be changed, by at least 50% of the default in either direction. | should | Auto | XAG 107 |
| I-14 | On a controller, A activates, B goes back, the shoulder buttons change the tab, and Start opens the menu. | must | Auto | Apple, Game controls |
| I-15 | The player can switch between the mouse, the keyboard and a controller at any time. The switch is immediate, and focus is kept. | must | Person | [Valve, Steam Deck recommendations](https://partner.steamgames.com/doc/steamdeck/recommendations) |

### Feedback and safety (F)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| F-1 | Every action that destroys something asks for confirmation and names what it destroys. Examples are selling an item, destroying an item, overwriting a save and deleting a save. | must | Auto for the confirmation. Person for its words. | XAG 107 and [AbleGamers, Accessible Player Experiences](https://accessible.games/accessible-player-experiences/) |
| F-2 | Where a confirmation would slow the player down too much, the action can be undone instead. | should | Person | XAG 107 |
| F-3 | In a confirmation, the button that has focus first is the one that destroys nothing. | should | Auto | Not from a guideline. A recommendation of the research. |
| F-4 | A disabled or locked control says why it is disabled. | must | Auto | XAG 114 |
| F-5 | Every action that changes something shows that it worked, at once or after a short delay the player can see. | must | Person | [Game Accessibility Guidelines](https://gameaccessibilityguidelines.com/full-list/) |
| F-6 | Settings are saved and kept. | must | Auto | Game Accessibility Guidelines |
| F-7 | A setting that changes what the player sees shows a preview of the change. | should | Auto for the preview | XAG 114 |

### Modern RPG screens (R)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| R-1 | When the player selects an item in the inventory or a store, the screen shows how the character's numbers would change compared with what the character wears in the same slot. | should | Person. Auto for the presence of the comparison. | Diablo II Resurrected, from press coverage only |
| R-2 | A list of items can be sorted, at least by name, by type and by how recently each item was found. | should | Auto for the sort control | Baldur's Gate 3 and Mass Effect Legendary Edition, from press coverage only |
| R-3 | A list of items can be filtered by type. | must | Auto for the filter control | XAG 112 |
| R-4 | A long list has a search or a way to jump quickly through it. | should | Auto | XAG 112 |
| R-5 | Items that are new since the player last looked are marked. | should | Person | A recommendation of the research. No standard was found. |
| R-6 | The player can see the current objective, be reminded of it, and hide it. | should | Person | Game Accessibility Guidelines, and [EA, Dragon Age: The Veilguard](https://www.ea.com/games/dragon-age/dragon-age-the-veilguard/news/accessibility-spotlight) |
| R-7 | A game term, such as Defense or Vitality, has its definition close at hand. | should | Person | XAG 114 |
| R-8 | An item's or a power's description shows which requirements the character does not meet, and not with colour alone. | must | Judge | XAG 102 |
| R-9 | In a store, the player can mark items as junk and sell them all at once. | should | Person | Mass Effect Legendary Edition, from press coverage only |
| R-10 | The player can save sets of equipment and switch between them. | should | Person | A recommendation of the research. No source was found. |

### Options and first launch (O)

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| O-1 | The accessibility options can be reached from the first screen at the first launch, and they have a page of their own. | must | Person | XAG 112 |
| O-2 | The default settings already meet T-1, T-2 and C-1, without the player changing anything. | must | Auto | XAG 112 |
| O-3 | kjk follows the system's text size, high contrast and caption settings where the platform has them. This includes Windows, macOS and Steam. | should | Person | XAG 102 and XAG 104 |
| O-4 | Each part of the HUD can be hidden on its own. | should | Auto for the settings | EA, Dragon Age: The Veilguard |
| O-5 | Each option has a one-line description in plain words. A value such as "minimal" is explained. | must | Auto for the description. Judge for its words. | XAG 104 and XAG 114 |
| O-6 | Camera shake, flashes and motion can be reduced. | must | Auto for the settings | Game Accessibility Guidelines |
| O-7 | Each page of options can be reset to its defaults. | should | Auto | A recommendation of the research. |

### KOTOR's feel (K)

No guideline gives numbers for feel. These rules turn the principles above into questions that a person or a judge can answer.

| ID | Rule | Level | Check | Source |
|---|---|---|---|---|
| K-1 | The screen keeps KOTOR's layout shape, which is where each region sits and how large it is next to the others, unless another rule forces a change. | must | Judge, shown KOTOR's screen and kjk's side by side | Principle 1 |
| K-2 | The screen keeps KOTOR's frame art, its colours and its sounds. | must | Judge | Principle 1 |
| K-3 | KOTOR's own fonts stay the default. A plain font may be offered next to them. | must | Person | Principle 4 |
| K-4 | Where a rule forces a change that players may dislike, the original form stays available with a setting. | must | Person | Principle 4 |
| K-5 | Every new helper can be switched off. | must | Auto for the setting | Principle 3 |
| K-6 | Every change to a screen names the rule on this page that it fixes, or has the owner's approval. | must | Person | Principle 7 |
| K-7 | A KOTOR 1 campaign's screens are KOTOR 2's layouts in KOTOR 1's colours (the "blue steel" skin), and a KOTOR 2 campaign's screens are KOTOR 2's. A change keeps working in both. | must | Judge | docs/UI.md, the skin |
| K-8 | New controls are built from the games' own art, fonts and sounds. Nothing is drawn by hand. | must | Person | The owner's rule, in the roadmap card https://github.com/kjk-engine/roadmap/issues/9 |
| K-9 | A reviewer can answer yes to "Does this still feel like KOTOR?". | must | Person | Principle 8 |

## Rules left out

- The research proposed a rule that KOTOR 1's screens look like KOTOR 1's. That does not fit kjk, because kjk draws a KOTOR 1 campaign with KOTOR 2's layouts in KOTOR 1's colours. K-7 says that instead.
- The research's rule on a "phone-sized" amount of elements per screen was left out. It cannot be measured, and L-5 measures the same problem with a number.

## Open questions

- **When the TV rules apply.** T-1 at TV distance, L-1 and L-2 are reported but not required until the owner decides.
- **T-6 and T-7.** The Xbox guidelines give 80 characters per line and a line spacing of 1.5 as defaults for players who cannot change the text. If kjk lets the player change the line length and the spacing, the two rules may be dropped. The owner decides.
- **A measure of layout shape.** K-1 is judged today. A measure that compares a screen's control rectangles with KOTOR's would turn it into an Auto check.
