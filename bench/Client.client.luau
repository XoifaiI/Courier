--!strict

local Framework = require("./Framework") :: any

local Runs = {
	Throughput = require("./Runs/Throughput"),
	Game = require("./Runs/Game"),
	Attack = require("./Runs/Attack"),
}

Runs[Framework.Config.Run].Client(Framework)
