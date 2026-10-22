--!strict

local Config = require("./Config")

local RUNS = {
	Throughput = require("./Runs/Throughput"),
	Game = require("./Runs/Game"),
	Attack = require("./Runs/Attack"),
	Cpu = require("./Runs/Cpu"),
}

RUNS[Config.Run].Client()
