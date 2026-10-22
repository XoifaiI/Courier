--!strict

local BitNet = (require("../Vendor/bitnet") :: any)({})

local TESTS = { "Booleans", "Dict", "Entities", "Numbers", "Strings", "Vectors" }

local Types = BitNet.types
local Byte = Types.u8

local Events: { [string]: any } = {
	Booleans = BitNet.event({ Args = Types.array(Types.bool) }),
	Entities = BitNet.event({
		Args = Types.array(
			Types.struct({ id = Byte, x = Byte, y = Byte, z = Byte, orientation = Byte, animation = Byte })
		),
	}),
	Strings = BitNet.event({ Args = Types.array(Types.string) }),
	Numbers = BitNet.event({ Args = Types.array(Byte) }),
	Dict = BitNet.event({ Args = Types.map(Types.string, Byte) }),
	Vectors = BitNet.event({ Args = Types.array(Types.vec3) }),
}

local GameEvents: { [string]: any } = {
	State = BitNet.event({
		Args = Types.array(Types.struct({ id = Types.u16, position = Types.vec3, yaw = Types.u16, health = Byte })),
	}),
	Hit = BitNet.event({ Args = Types.struct({ attacker = Types.u16, target = Types.u16, damage = Byte }) }),
	Pickup = BitNet.event({ Args = Types.struct({ item = Types.u16, slot = Byte }) }),
	Chat = BitNet.event({ Args = Types.string }),
	Input = BitNet.event({
		Args = Types.struct({
			move = Types.vec3,
			look = Types.vec3,
			jump = Types.bool,
			fire = Types.bool,
			crouch = Types.bool,
		}),
	}),
}

local Fire: { [string]: (any) -> () } = {}
for Test, Target in Events do
	Fire[Test] = function(Value: any)
		Target:FireServer(Value)
	end
end

local Send: { [string]: (Player, any) -> () } = {}
for _, Name in { "State", "Hit", "Pickup", "Chat" } do
	local Target = GameEvents[Name]
	Send[Name] = function(Player: Player, Value: any)
		Target:FireClient(Player, Value)
	end
end

return {
	Tests = TESTS,
	Fire = Fire,
	On = function(Test: string, Handler: (Player, any) -> ())
		Events[Test].OnServerEvent:Connect(Handler)
	end,
	Game = {
		Send = Send,
		On = function(Name: string, Handler: (any) -> ())
			GameEvents[Name].OnClientEvent:Connect(Handler)
		end,
		Input = function(Value: any)
			GameEvents.Input:FireServer(Value)
		end,
		OnInput = function(Handler: (Player, any) -> ())
			GameEvents.Input.OnServerEvent:Connect(Handler)
		end,
	},
}
