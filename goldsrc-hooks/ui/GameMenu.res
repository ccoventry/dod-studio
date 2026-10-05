// Written by DoD Studio: its main menu, read with -addons over the game's own
// (dod\resource\GameMenu.res, which is never written). DoD Studio rewrites
// this file when it changes; remove this line to keep your own edits.
"GameMenu"
{
	"1"
	{
		"label" "DoD Studio"
		"command" "engine dodstudio_panel 1"
	}
	"2"
	{
		"label" ""
		"command" ""
	}
	"3"
	{
		"label" "#GameUI_GameMenu_ResumeGame"
		"command" "ResumeGame"
		"OnlyInGame" "1"
	}
	"4"
	{
		"label" "#GameUI_GameMenu_Disconnect"
		"command" "Disconnect"
		"OnlyInGame" "1"
		"notsingle" "1"
	}
	"5"
	{
		"label" ""
		"command" ""
		"OnlyInGame" "1"
	}
	"6"
	{
		"label" "#GameUI_GameMenu_Options"
		"command" "OpenOptionsDialog"
	}
	"7"
	{
		"label" "#GameUI_GameMenu_Quit"
		"command" "Quit"
	}
}
