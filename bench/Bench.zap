opt client_output = "vendor/zap/Client.luau"
opt server_output = "vendor/zap/Server.luau"
opt write_checks = false

type Entity = struct {
	id: u8,
	x: u8,
	y: u8,
	z: u8,
	orientation: u8,
	animation: u8,
}

event Booleans = {
	from: Client,
	type: Reliable,
	call: SingleSync,
	data: boolean[..1000],
}

event Entities = {
	from: Client,
	type: Reliable,
	call: SingleSync,
	data: Entity[..1000],
}

event Strings = {
	from: Client,
	type: Reliable,
	call: SingleSync,
	data: string.binary(..50)[..500],
}

event Numbers = {
	from: Client,
	type: Reliable,
	call: SingleSync,
	data: u8[..2000],
}

event Dict = {
	from: Client,
	type: Reliable,
	call: SingleSync,
	data: map { [string.binary(..50)]: u8 },
}

event Vectors = {
	from: Client,
	type: Reliable,
	call: SingleSync,
	data: vector[..500],
}

type Unit = struct {
	id: u16,
	position: vector,
	yaw: u16,
	health: u8,
}

event State = {
	from: Server,
	type: Reliable,
	call: SingleSync,
	data: Unit[..64],
}

event Hit = {
	from: Server,
	type: Reliable,
	call: SingleSync,
	data: struct {
		attacker: u16,
		target: u16,
		damage: u8,
	},
}

event Pickup = {
	from: Server,
	type: Reliable,
	call: SingleSync,
	data: struct {
		item: u16,
		slot: u8,
	},
}

event Chat = {
	from: Server,
	type: Reliable,
	call: SingleSync,
	data: string.binary(..200),
}

event Input = {
	from: Client,
	type: Reliable,
	call: SingleSync,
	data: struct {
		move: vector,
		look: vector,
		jump: boolean,
		fire: boolean,
		crouch: boolean,
	},
}
