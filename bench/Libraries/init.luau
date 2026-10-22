--!strict

export type Game = {
	Send: { [string]: (Player, any) -> () },
	On: (Name: string, Handler: (any) -> ()) -> (),
	Input: (Value: any) -> (),
	OnInput: (Handler: (Player, any) -> ()) -> (),
}

export type Library = {
	Tests: { string },
	Fire: { [string]: (Value: any) -> () },
	On: (Test: string, Handler: (Player, any) -> ()) -> (),
	Game: Game,
}

type Entry = { Vendor: string?, Load: () -> Library }

local VENDOR = (script.Parent :: Instance):WaitForChild("Vendor")

local ENTRIES: { [string]: Entry } = {
	BitNet = {
		Vendor = "bitnet",
		Load = function(): Library
			return require("@self/BitNet")
		end,
	},
	Blink = {
		Vendor = "blink",
		Load = function(): Library
			return require("@self/Blink")
		end,
	},
	BlinkBlox = {
		Vendor = "blinkblox",
		Load = function(): Library
			return require("@self/BlinkBlox")
		end,
	},
	ByteNet = {
		Vendor = "bytenet",
		Load = function(): Library
			return require("@self/ByteNet")
		end,
	},
	Courier = {
		Vendor = "net",
		Load = function(): Library
			return require("@self/Courier")
		end,
	},
	Lync = {
		Vendor = "lync",
		Load = function(): Library
			return require("@self/Lync")
		end,
	},
	NetRayCompile = {
		Vendor = "netraycompile",
		Load = function(): Library
			return require("@self/NetRayCompile")
		end,
	},
	Packet = {
		Vendor = "packet",
		Load = function(): Library
			return require("@self/Packet")
		end,
	},
	PakNet = {
		Vendor = "paknet",
		Load = function(): Library
			return require("@self/PakNet")
		end,
	},
	QuickNet = {
		Vendor = "quicknet",
		Load = function(): Library
			return require("@self/QuickNet")
		end,
	},
	Roblox = {
		Vendor = nil,
		Load = function(): Library
			return require("@self/Roblox")
		end,
	},
	Satset = {
		Vendor = "satset",
		Load = function(): Library
			return require("@self/Satset")
		end,
	},
	Squash = {
		Vendor = "squash",
		Load = function(): Library
			return require("@self/Squash")
		end,
	},
	Zap = {
		Vendor = "zap",
		Load = function(): Library
			return require("@self/Zap")
		end,
	},
}

local Loaded: { [string]: Library } = {}

local function Available(): { string }
	local Names = {}
	for Name, Entry in ENTRIES do
		if Entry.Vendor == nil or VENDOR:FindFirstChild(Entry.Vendor) then
			table.insert(Names, Name)
		end
	end
	table.sort(Names)
	return Names
end

local function Load(Name: string): Library
	local Library = Loaded[Name]
	if Library == nil then
		local Entry = ENTRIES[Name]
		if Entry == nil then
			error(`Bench: there is no library called {Name}`)
		end
		Library = Entry.Load()
		Loaded[Name] = Library
	end
	return Library
end

return {
	Available = Available,
	Load = Load,
}
