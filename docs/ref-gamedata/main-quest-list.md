> Titles are the live English display strings as of **Crimson Desert 2.02**, and every row is tied to the game
> row its title comes from (`MissionKey` / `QuestKey`) in
> [`src/c_abi/main_quest_chapter.rs`](../../src/c_abi/main_quest_chapter.rs). The live-install test
> `curated_titles_match_live_install` fails when a key's title drifts, so future retitles surface on the next
> patch run. See [Reconciliation against 2.02](#reconciliation-against-202) for what changed and why.

### **Prologue: Dead of Night**

* Ambush
* Unfamiliar Land
* In Ashes
* Unknown Space *(no counterpart in 2.02)*
* Realm of Uncertainty
* New Journey

### **Chapter 1: The First Encounter**

* **Trials of Kindness**
* Where Rumors Gather
* A Mysterious Beggar
* True Wisdom in Kindness
* Actions Speak Volumes
* A Transcendent Connection


* **Traces**
* Mystical Key
* Faced With the Truth
* The Faltering Abyss
* Woman in White



### **Chapter 2: Golden Greed**

* **Unexpected Gift**
* Where the Light Leads
* Memory Fragment
* Reunion


* **Hernand in Chaos**
* For Honor
* Awestruck
* Shadow Over the River
* A Collection of Woes
* Trial After Trial
* Down in the Muck
* Missing Companion
* Shadowed Secrets


* **The End of Greed**
* The Dark Veil
* The Flames of Greed
* Kidnapped Healer
* Rebellion or Revolution
* Resounding Victory



### **Chapter 3: Howling Hill**

* **Homestead**
* Old Friend
* First Step to Rebuilding
* A Fresh Start
* A Well-Earned Meal
* Comrade's Return
* Familiar Curses *(no counterpart in 2.02)*


* **The Face Behind the Mask**
* Homecoming
* Traces in the Manor
* Inhuman
* Seed of Dread
* Dance with the Devil


* **Pioneering**
* The Storm Passes
* Scattered Comrades
* Rumors from the Sawmill
* A Gentle Touch
* Commotion at Howling Hill
* Greymanes Reunited



### **Chapter 4: The Price of Knowledge**

* **Mysterious Pot**
* Kilnden Workshop
* Kilnden Kiln Repair
* The Mysterious Pot
* Pot Put to Good Use


* **Daily Life**
* Ruckus in the Arena
* Skilled in Archery


* **Forbidden Knowledge**
* The Words of Alustin
* Scholastone
* On the Right Path
* Gate to the Otherworld
* Spire of the Stars
* Obsession and Madness
* A Looming Shadow



### **Chapter 5: Guest Unbidden**

* **Uninvited Guest**
* Ulterior Motives
* Unwelcomed Guests
* Demenissian Delegation
* Exposed Plot


* **Black and White**
* The Missing Seal
* Crowcaller
* The Crow's Warning
* Blood on the Wind
* The Church's Hidden Secret
* Approaching the Nest



### **Chapter 6: Cracks in the Shield**

* **Blazing Beacon**
* News Arrives
* To the Battlefield
* The Counterattack


* **Below the Banners**
* Pike Again *(no counterpart in 2.02)*


* **Cradle of Defense**
* Hand of Deliverance
* Fire on the Front Line


* **Turning Tides**
* Fire Support
* In Ashes
* Hidden Fangs
* Reclamation


* **The Unyielding Shields**
* A Thousand Troops
* Traitor
* All Quiet on the Front
* News of Victory
* Return Home



### **Chapter 7: Homecoming**

* **Morning Mist**
* Ashes of Treachery
* Trust Lost
* Bared Fangs
* Rekindled Hope
* A Stand of Resolve


* **Dawn**
* Shadows Over Pailune
* Driving out the Shadows
* Lurking Wolves
* Reclamation
* Lonely Jackals
* Resolution


* **Decisive Battle**
* The Counterattack
* Unleashed Fury
* The Final Bridge
* Broken Claws *(no counterpart in 2.02)*
* Battle at Silver Wolf Mountain
* Hollow Victory


* **Twisted Fate**
* Ludvig's Whereabouts
* Time to Face Justice



### **Chapter 8: Blood Coronation**

* **Ashen Steps**
* The Unending Pursuit
* Bonds
* Ritual Preparations
* Where the Wind Guides You


* **Demeniss Bound**
* Chasing a Shadow
* Blazing Fire
* Murmurs in the Dark
* Signed in Blood
* Steadfast in the Storm
* Preparing to Strike
* Quelling the Uprising
* The Cursed Knight
* The Blood Coronation


* **Traitor**
* The Thread
* A Fleeting Dream



### **Chapter 9: The Sage of the Desert**

* **The Calling**
* An Unknown Voice
* Cloister of Enlightenment


* **Shattered Ties**
* The Spear's Mark
* Shackles of Fate


* **Thinning Blade**
* The Crossroads
* Unwavering Steps


* **Six Statues and the Beast**
* The Shroud of Dawn
* Jijeong Temple in Chaos
* Path to Enlightenment
* Path of the Disciple
* True Strength
* Confronting What Lies Within


* **Veiled Witch**
* Fragments of Darkness
* Pursuit Beyond the Veil
* Black Witch


* **Enlightenment**
* The Cloister of Enlightenment
* The Sage of the Desert
* New Perspectives
* Lust for Power *(no counterpart in 2.02)*



### **Chapter 10: Counterattack**

* **Secret Weapon**
* A New Front
* The Gate of War
* Master of the Ironworks
* Hidden Ace
* Clockwork Insect Clash


* **Greater Firepower**
* Beating Heart
* Invaders from the East
* Cold-Hearted Hunter
* Lingering Shadow



### **Chapter 11: Truth and Reality**

* **Brave New World**
* The City of Steel
* At a Crossroads
* Strange Manor
* Fortress Keys
* Truth and Lies


* **Foreboding Shadow**
* Master of a Forgotten Land
* Whispers in the Wind
* Flying Fortress Orbian



### **Chapter 12: The Abyss**

* **The Final Battle**
* Precise Execution
* Deferred Advance
* Departure of the Brave
* Forbidden Gate


* **The Void**
* A Shadow in the Void
* Blinding Darkness



### **Epilogue: Journey's End**

* **Journey's End**
* A New Beginning
* Peace Restored
* The Unyielding Shield
* The Heart of Pywel
* The Enduring Flame
* The Evolving City
* The Desert's Edge
* New Horizons


## Reconciliation against 2.02

This list was transcribed from a wiki. On 2.02, 55 of its 170 rows and 9 of its 38 arc headings did not
resolve to the live title of their mission / quest: 50 rows and 8 arcs are re-paired below, 5 rows have no
counterpart at all, and one arc heading turned out to be a mission title. Each re-pairing uses the live row of
the **same quest family** (the arc's `Mission_<Family>_*` / `Quest_<Family>_*` internal names), with the title
only picking within that family. `Row` is the 0-based index into `ROWS`.
A few old titles were not mission titles at all but **stage** titles (stageinfo, same `0x101` namespace)
whose stage the chosen mission references — those pairings are deterministic.

| Row | Was | Now | Game row | Evidence |
|---|---|---|---|---|
| 9 | Actions Speak Louder than Words | Actions Speak Volumes | `1000042` `Mission_MeetAlustain_Alchemist_Cleaning` | same arc family, same meaning |
| 10 | Heart Beyond Borders | A Transcendent Connection | `1000051` `Mission_MeetAlustain_Alustain_CatchCat` | inferred — only other unclaimed `Mission_MeetAlustain_*` row of the arc; same sense (a transcendent connection / a heart beyond borders) |
| 12 | Polar Opposites | Faced With the Truth | `1000048` `Mission_MeetAlustain_Alustain_WhiteWolf` | stage `MeetAlustain_Alustain_GreatLibrary` (titled "Polar Opposites") is referenced only by this mission's steps `_2` / `_3` |
| 13 | Abyss Without Balance | The Faltering Abyss | `1000546` `Mission_MeetAlustain_Alustain_AbyssGate` | same arc family, same meaning |
| 20 | Shadow Cast Over the River | Shadow Over the River | `1000529` `Mission_SplitHorn_Boss_SpringtideWatermill` | same arc family, same meaning |
| 21 | Where Misery Gathers | A Collection of Woes | `1000517` `Mission_SplitHorn_Boss_HernandRequestBoard` | same arc family, same meaning |
| 23 | The Man Trapped in the Mire | Down in the Muck | `1000180` `Mission_SplitHorn_Boss_Ibano_FirstMeet` | same arc family, same meaning |
| 25 | Secrets Hidden in the Dark | Shadowed Secrets | `1000183` `Mission_SplitHorn_Boss_ThiefCave` | same arc family, same meaning |
| 30 | Cheers Echoing From the Edge | Resounding Victory | `1000193` `Mission_SplitHorn_Boss_Battle` | same arc family, same meaning |
| 34 | Reward for Their Sweat | A Well-Earned Meal | `1000219` `Mission_GreyWolf_Camp_RepairCamp_Cook` | same arc family, same meaning |
| 35 | Return of the Comrade | Comrade's Return | `1000220` `Mission_GreyWolf_Camp_Join_Marius` | same arc family, same meaning |
| 37 | Return | Homecoming | `1000504` `Mission_Grace_Dominion_Dwayne` | inferred — only unclaimed `Mission_Grace_Dominion_*` row; return ≈ homecoming |
| 39 | Nonhuman | Inhuman | `1000235` `Mission_Grace_Dominion_ReedDevilWitness` | same arc family, same meaning |
| 40 | Seed of Unease | Seed of Dread | `1001417` `Mission_Grace_Dominion_ReedDevil_ScareCrow` | same arc family, same meaning |
| 42 | Hope After the Draught | The Storm Passes | `1001018` `Mission_ForGraymane` | inferred — `Mission_ForGraymane` is the base of the arc's own ForGraymane chain; hope after hardship ≈ the storm passes |
| 46 | Bustling Hill | Commotion at Howling Hill | `1000852` `Graymane_ExpandCamp_Lv1` | inferred — `Graymane_ExpandCamp_Lv1`; bustling hill ≈ commotion at Howling Hill |
| 49 | Kiln Repair at the Kilnden Workshop | Kilnden Kiln Repair | `1000013` `Mission_KukuBird_Kuku_Repairkuku` | stage `KukuBird_Kuku_Repairkuku_GuideText_Background` is referenced by this mission |
| 51 | The Iron Pot's Usage | Pot Put to Good Use | `1000225` `Mission_KukuBird_Kuku_StoneSeal` | same arc family, same meaning |
| 52 | Disturbance at the Arena | Ruckus in the Arena | `1001219` `Mission_GreymaneCamp_Contents_Fight` | same arc family, same meaning |
| 60 | Casted Shadow | A Looming Shadow | `1000269` `Mission_TrollUniversity_Brain_Lost_MasterGrundir` | "Cast Shadow" is the title of stage `TrollUniversity_Brain_Lost_MasterGrundir`, referenced by this mission's step `_0` |
| 61 | Double-sided Invitation | Ulterior Motives | `1000432` `Mission_Imp_Banquet_Boss_Invite` | same arc family, same meaning |
| 68 | Bloodwind | Blood on the Wind | `1000245` `Mission_Crowman_Boss_Ribentain_Battle` | same arc family, same meaning |
| 69 | Secret at the Church | The Church's Hidden Secret | `1000247` `Mission_Crowman_Boss_Ribentain_HideToken` | same arc family, same meaning |
| 70 | Toward the Nest (Spire of Soaring) | Approaching the Nest | `1000248` `Mission_Crowman_Boss_Battle` | same arc family, same meaning |
| 71 | News | News Arrives | `1000137` `Mission_Silver_Armor_Boss_Celester_VisitorToCamp` | same arc family, same meaning |
| 75 | The Touch of Deliverance | Hand of Deliverance | `1000419` `Mission_Silver_Armor_Boss_Occupation_A` | same arc family, same meaning |
| 76 | Fire on the Frontlines | Fire on the Front Line | `1000161` `Mission_Silver_Armor_Boss_Occupation_ThalwyndVillage` | same arc family, same meaning |
| 88 | Bared Fang | Bared Fangs | `1001692` `Mission_Ludvig_Boss_Investigation` | same arc family, same meaning |
| 90 | Podium of Resolve | A Stand of Resolve | `1000134` `Mission_Beighen_Basketmaker_Tolstein_Speech` | same arc family, same meaning |
| 101 | Battle at Silverwolf Mountain | Battle at Silver Wolf Mountain | `1000258` `Mission_Mjordin_Boss_LavaMountain` | same arc family, same meaning |
| 102 | Incomplete Victory | Hollow Victory | `1000259` `Mission_Mjordin_Boss_UncomfortableEnding_Return_I` | "Incomplete Victory" is the title of stage `Mjordin_Boss_UncomfortableEnding_Return_I`, referenced by this mission's step `_0` |
| 105 | Healing Pailune | The Unending Pursuit | `1000092` `Mission_BloodCoronation_TolsteinGuide` | inferred — the arc's own quest is `Quest_BloodCoronation_TolsteinGuide`; weak title match |
| 106 | A Bond | Bonds | `1001213` `Mission_BloodCoronation_EastWitch` | inferred — `Mission_BloodCoronation_*` row; a bond ≈ bonds |
| 111 | Whispering Shadows | Murmurs in the Dark | `1000212` `Mission_BloodCoronation_Marseille` | same arc family, same meaning |
| 112 | Bloodied Invitation | Signed in Blood | `1000388` `Mission_BloodCoronation_Byron` | inferred — `Mission_BloodCoronation_*` row; bloodied invitation ≈ signed in blood |
| 113 | Resolve Amidst a Storm | Steadfast in the Storm | `1000276` `Mission_BloodCoronation_Azerian` | same arc family, same meaning |
| 114 | Preparations for Advance | Preparing to Strike | `1000135` `Mission_ThornRoseFort_Block_Start` | same arc family, same meaning |
| 115 | Rebel Suppression | Quelling the Uprising | `1001390` `Mission_ThornRoseFort_Liberation` | same arc family, same meaning |
| 118 | Clue | The Thread | `1000271` `Mission_BloodCoronation_Azerian_Dead` | inferred — `Mission_BloodCoronation_*` row; clue ≈ the thread |
| 122 | Mark of the Scar | The Spear's Mark | `1000159` `Goblin_Master_Doo_Urdavah` | same arc family, same meaning |
| 124 | Crossing Point | The Crossroads | `1000162` `Goblin_Master_Doo_OldKliff` | same arc family, same meaning |
| 126 | Morning Fog | The Shroud of Dawn | `1000158` `Goblin_Master_Doo_Jijeongtemple` | same arc family, same meaning |
| 131 | Face the Inner Self | Confronting What Lies Within | `1002834` `Goblin_Master_Doo_Jijeongtemple_End` | same arc family, same meaning |
| 139 | Untouchable | A New Front | `1000223` `Mission_MarniDragon_Boss_Meet_Yann` | inferred — last unclaimed non-DLC `Mission_MarniDragon_*` row; key 1000223 sits right before the arc's next mission (1000224) |
| 146 | Frozen Hearted Predator | Cold-Hearted Hunter | `1000232` `Mission_MarniDragon_Boss_SteelMillEscape` | same arc family, same meaning |
| 149 | Crossroads | At a Crossroads | `1000616` `Mission_MarniDragon_Boss_Underground` | same arc family, same meaning |
| 155 | Cloud Fortress Orbian | Flying Fortress Orbian | `1000114` `Mission_MarniDragon_Boss_GoldenStar_AirCastle` | same arc family, same meaning |
| 163 | Peace in Hernand | Peace Restored | `1000531` `Mission_Caliburn_Boss_Ending_ComeBackHernand` | same arc family, same meaning |
| 164 | The Unyielding Shields | The Unyielding Shield | `1000532` `Mission_Caliburn_Boss_Ending_ComeBackCalphade` | same arc family, same meaning |
| 167 | Evolving City | The Evolving City | `1001690` `Mission_Caliburn_Boss_Ending_Delesyia` | same arc family, same meaning |
| arc | Trace | Traces | `1000509` `Quest_MeetAlustain_Alchemist_CrowWing` | same quest family, same meaning |
| arc | Mysterious Iron Pot | Mysterious Pot | `1000142` `Quest_KukuBird_Kuku` | same quest family, same meaning |
| arc | Under the Banner | Below the Banners | `1000781` `Quest_Silver_Armor_Boss_SecuringBase` | inferred — the arc's missions sit in this quest's family |
| arc | The Undying Shields | The Unyielding Shields | `1000112` `Quest_Silver_Armor_Boss_Battle` | same quest family, same meaning |
| arc | Dawn Mist | Morning Mist | `1000579` `Quest_Ludvig_Boss_DawnFog` | same quest family, same meaning |
| arc | Dawnrise | Dawn | `1000597` `Quest_Ludvig_Boss_Jackal` | inferred — the arc's missions sit in this quest's family |
| arc | To Demeniss | Demeniss Bound | `1000725` `Quest_BloodCoronation_PlaceOutOfLight` | same quest family, same meaning |
| arc | Six Pensive Statues and the Evil Spirit | Six Statues and the Beast | `1000305` `Quest_Goblin_Master_Doo_Trial_JijeongTemple` | same quest family, same meaning |

No live mission, quest or stage title corresponds to these, so they are kept as transcribed and
carry no key (`Unresolved`); the title lookups still answer for them:

- #3 Unknown Space (Prologue: Dead of Night)
- #36 Familiar Curses (Chapter 3: Howling Hill / Homestead)
- #74 Pike Again (Chapter 6: Cracks in the Shield / Under the Banner)
- #100 Broken Claws (Chapter 7: Homecoming / Decisive Battle)
- #138 Lust for Power (Chapter 9: The Sage of the Desert / Enlightenment)

"Cradle of Defense" is kept as an arc heading, but its title belongs to a mission
(`1001231` `Mission_Silver_Armor_Boss_Occupation_Refinery`), not a quest.
