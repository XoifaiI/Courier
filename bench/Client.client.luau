--!strict

local Config = require("./Config")

local RUNS = {
	Throughput = require("./Runs/Throughput"),
	Attack = require("./Runs/Attack"),
}

RUNS[Config.Run].Client()
