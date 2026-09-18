-----

中文部份只是參考，非實際遊戲內文字

-----

### **Scattered Embers (散落的灰燼派系)**

* quest: Record of the Greymanes, faction: Scattered Embers
* quest: Strongbox with Wheels, faction: Scattered Embers
* quest: Brightening the Spirits, faction: Scattered Embers
* quest: Chance to Make a Fortune, faction: Scattered Embers
* quest: Rescuing the Pailunese Refugees, faction: Scattered Embers
* quest: The Greymanes' New Fangs, faction: Scattered Embers
* quest: The Nag and the Stubborn One, faction: Scattered Embers
* quest: A Chunk of Meat, faction: Scattered Embers
* quest: Fang Without a Master, faction: Scattered Embers
* quest: Letter at the Shrine, faction: Scattered Embers
* quest: Running Loot, faction: Scattered Embers
* quest: Empty Wagon, faction: Scattered Embers
* quest: Gloomy Gray, faction: Scattered Embers
* quest: Vibrant Dye, faction: Scattered Embers
* quest: A Fresh Color, faction: Scattered Embers
* quest: Shattered Charmed Life, faction: Scattered Embers
* quest: A Move on the Table, faction: Scattered Embers
* quest: Liquor and Memories, faction: Scattered Embers
* quest: Trembling Hands, faction: Scattered Embers
* quest: White Wood Bow, faction: Scattered Embers
* quest: The New Archers, faction: Scattered Embers
* quest: Face on the Bounty Notice, faction: Scattered Embers
* quest: Plenty of Bounty, faction: Scattered Embers
* quest: The Cost of the Tab, faction: Scattered Embers
* quest: Logging Without an Axe, faction: Scattered Embers
* quest: Quarrel on Horseback, faction: Scattered Embers
* quest: Showdown in the Saddles, faction: Scattered Embers
* quest: Scent of Gold, faction: Scattered Embers

### **Grounds of the Sunrise (朝陽之地派系)**

* quest: Embers of Return, faction: Grounds of the Sunrise
* quest: Reuniting with Comrades, faction: Grounds of the Sunrise
* quest: For a Better Tomorrow, faction: Grounds of the Sunrise

### **Greymane Commissions (灰鬃委託派系)**

* quest: Carl's Request, faction: Greymane Commissions
* quest: Ronnie's Request, faction: Greymane Commissions
* quest: Ross's Request, faction: Greymane Commissions
* quest: Tranan's Request, faction: Greymane Commissions
* quest: Brice's Request, faction: Greymane Commissions
* quest: Ronald's Request, faction: Greymane Commissions
* quest: Pierce's Request, faction: Greymane Commissions

### **House Celeste (塞萊斯特家族 - 懸賞任務)**

* quest: Bounty Notice - Jeffrey, faction: House Celeste
* quest: Bounty Notice - Bianca, faction: House Celeste
* quest: Bounty Notice - Simon de Montfort, faction: House Celeste
* quest: Bounty Notice - Alessio, faction: House Celeste

### **House Roberts (羅伯茨家族派系)**

* quest: Estate in Dismay, faction: House Roberts
* quest: Continuing Concern, faction: House Roberts
* quest: Boulder from the Sky, faction: House Roberts

### **Hernand Commissions (赫爾南德委託派系)**

* quest: Serge's Request, faction: Hernand Commissions
* quest: Break in the grindstone, faction: Hernand Commissions
* quest: Lunchbox of Love, faction: Hernand Commissions
* quest: The Weight of Knowledge, faction: Hernand Commissions
* quest: Rhett's Request, faction: Hernand Commissions
* quest: Renee's Request, faction: Hernand Commissions
* quest: Turnali's Request, faction: Hernand Commissions
* quest: Prox's Request, faction: Hernand Commissions
* quest: Tina's Request, faction: Hernand Commissions
* quest: Bruna's Request, faction: Hernand Commissions
* quest: Ugmon's Request, faction: Hernand Commissions

### **Hernand Requests (赫爾南德請求派系)**

* quest: Goddess of Abundance, faction: Hernand Requests
* quest: Path that Connects to House of Healing, faction: Hernand Requests
* quest: Wolf Protecting Hernand, faction: Hernand Requests
* quest: A Favor for Hernand, faction: Hernand Requests
* quest: Bells Ringing Again, faction: Hernand Requests

### **其他各大勢力與派系任務（Other Factions）**

* quest: Trembling Woods, faction: Pororin Forest Guardians
* quest: House of Spears, faction: House Alfonso
* quest: Lord Amidst the Ruins, faction: House Serkis
* quest: Deathchime, faction: House Wells
* quest: Mushrooms Growing Among Poison, faction: Demeniss Commissions
* quest: Crossroads of Succession, faction: Pailune Militia
* quest: Antumbra's Sword, faction: Antumbra Order
* quest: The Witch of Wisdom, faction: Antumbra Order
* quest: Veil of the Yard, faction: Giant's Yard
* quest: Encirclement on the Cliff, faction: Giant's Yard
* quest: Dangerous Saltroad, faction: Goldenscales on the Saltroad
* quest: Siege of the Abandoned Castle Ruins, faction: Hunters of the Abandoned Castle Ruins
* quest: Veil of the Abandoned Castle Ruins, faction: Hunters of the Abandoned Castle Ruins
* quest: The Fangs That Devoured the Village, faction: The Fangs Beneath the Rock
* quest: The Gorge Under Siege, faction: Those Who Constrict the Research Expedition
* quest: Rainforest Gorge, faction: Those Who Constrict the Research Expedition
* quest: The Missing Desert Melons, faction: Harvest of Greed
* quest: A Village of Growing Suspicion, faction: Tales of the Crimson Desert Merchants
* quest: Thomas's Request, faction: Tales of the Crimson Desert Merchants
* quest: Between Drinks and Cheers, faction: Tales of the Crimson Desert Residents
* quest: Friend's Whereabouts, faction: Tales of the Crimson Desert Residents
* quest: Dirty Marauders, faction: Tales from the Corners of Crimson Desert
* quest: Futile Goodwill, faction: Tales from the Corners of Crimson Desert

## Reconciliation against 2.02

Titles are the live English display strings as of Crimson Desert 2.02; each row is tied to its game row in
[`src/c_abi/side_quest_faction.rs`](../../src/c_abi/side_quest_faction.rs). The list follows the in-game
journal, so most entries are **missions** (`MissionKey`, PALOC `0x101`) and 20 are quests (`QuestKey`,
`0x100`). On 2.02 these 9 titles matched nothing live and were corrected:

| Was | Now | Game row |
|---|---|---|
| To the Rescue | Rescuing the Pailunese Refugees | mission `1001412` `Mission_GreymaneCamp_Contents_RescueRefugees` — inferred (same `Mission_GreymaneCamp_Contents_*` family) |
| Bounty Target: Jeffrey | Bounty Notice - Jeffrey | mission `1000833` `Mission_Hernand_Wanted_Guide` |
| Bounty Target: Bianca | Bounty Notice - Bianca | mission `1000349` `Mission_Her_Wanted_Criminal_Bianca` |
| Bounty Target: Simon de Montfort | Bounty Notice - Simon de Montfort | mission `1000344` `Mission_Her_Wanted_Criminal_Simon_de_Montfort` |
| Bounty Target: Alessio | Bounty Notice - Alessio | mission `1000347` `Mission_Her_Wanted_Criminal_Alessio` |
| The Trembling Woods | Trembling Woods | quest `1000159` `Quest_Node_Her_PororinVillage_Trembling_Woods_Ent_Normal` |
| Mushrooms Growing Among Poisons | Mushrooms Growing Among Poison | quest `1000615` `Quest_Node_Dem_JijeongTemple_HiddenCave_Normal` |
| Encirlement on the Cliff | Encirclement on the Cliff | mission `1002130` `Mission_Node_Crim_GiantsYardForwardCamp_Block_SubNode` |
| The Fangs that Devoured the Village | The Fangs That Devoured the Village | mission `1000130` `Mission_RockVillage_Block_Start` |

## Retitles in 2.03

2.03's English copy pass changed one title in this list; its key did not change.

| 2.02 | 2.03 | Game row |
|---|---|---|
| Breaking in the Grindstone | Break in the grindstone | mission `1000015` `Mission_HernandCastle_SpinStone` (the lower-case "grindstone" is the game's) |
