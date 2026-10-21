--!strict

local Satset = require("../../Vendor/satset") :: any

local Types = Satset.Types

Satset.start()

local Packets: { [string]: any } = {
	Booleans = Satset.definePacket({
		name = "Booleans",
		schema = { Value = Types.array(Types.bool) },
		reliable = true,
	}),
	Entities = Satset.definePacket({
		name = "Entities",
		schema = {
			Value = Types.array(Satset.struct({
				id = Types.u8,
				x = Types.u8,
				y = Types.u8,
				z = Types.u8,
				orientation = Types.u8,
				animation = Types.u8,
			})),
		},
		reliable = true,
	}),
	Strings = Satset.definePacket({
		name = "Strings",
		schema = { Value = Types.array(Types.string8) },
		reliable = true,
	}),
	Numbers = Satset.definePacket({
		name = "Numbers",
		schema = { Value = Types.array(Types.u8) },
		reliable = true,
	}),
	Dict = Satset.definePacket({
		name = "Dict",
		schema = { Value = Types.map(Types.string8, Types.u8) },
		reliable = true,
	}),
	Vectors = Satset.definePacket({
		name = "Vectors",
		schema = { Value = Types.array(Types.Vector3) },
		reliable = true,
	}),
}

local GamePackets: { [string]: any } = {
	State = Satset.definePacket({
		name = "State",
		schema = {
			Value = Types.array(Satset.struct({
				id = Types.u16,
				position = Types.Vector3,
				yaw = Types.u16,
				health = Types.u8,
			})),
		},
		reliable = true,
	}),
	Hit = Satset.definePacket({
		name = "Hit",
		schema = { attacker = Types.u16, target = Types.u16, damage = Types.u8 },
		reliable = true,
	}),
	Pickup = Satset.definePacket({
		name = "Pickup",
		schema = { item = Types.u16, slot = Types.u8 },
		reliable = true,
	}),
	Chat = Satset.definePacket({
		name = "Chat",
		schema = { Value = Types.string8 },
		reliable = true,
	}),
	Input = Satset.definePacket({
		name = "Input",
		schema = {
			move = Types.Vector3,
			look = Types.Vector3,
			jump = Types.bool,
			fire = Types.bool,
			crouch = Types.bool,
		},
		reliable = true,
	}),
}

local Wrapped: { [string]: boolean } = { State = true, Chat = true }

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local Packet = GamePackets[Name]
	if Wrapped[Name] then
		local Payload: { Value: any } = { Value = nil }
		Send[Name] = function(Player: Player, Data: any)
			Payload.Value = Data
			Packet:fireClient(Player, Payload)
		end
	else
		Send[Name] = function(Player: Player, Data: any)
			Packet:fireClient(Player, Data)
		end
	end
end

local Fire: { [string]: (any) -> () } = {}
for Name, Packet in Packets do
	local Payload: { Value: any } = { Value = nil }
	Fire[Name] = function(Data: any)
		Payload.Value = Data
		Packet:fireServer(Payload)
	end
end

return {
	Fire = Fire,
	On = function(Name: string, Callback: (Player, any) -> ())
		Packets[Name]:listenServer(function(Player: Player, Data: any)
			Callback(Player, Data.Value)
		end)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Callback: (any) -> ())
			if Wrapped[Name] then
				GamePackets[Name]:listen(function(Data: any)
					Callback(Data.Value)
				end)
			else
				GamePackets[Name]:listen(function(Data: any)
					Callback(Data)
				end)
			end
		end,
		Fire = {
			Input = function(Data: any)
				GamePackets.Input:fireServer(Data)
			end,
		},
		Listen = function(Name: string, Callback: (Player, any) -> ())
			GamePackets[Name]:listenServer(Callback)
		end,
	},
}
