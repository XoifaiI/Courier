local EMPTY_LIST: { any } = table.freeze({} :: { any })
local EMPTY_HANDLE = InstanceHandle.new(nil)
local HANDLES: { [any]: InstanceHandle } = setmetatable({}, { __mode = "k" }) :: any

local function NewHandle(Value: Instance?): InstanceHandle
	if Value == nil then
		return EMPTY_HANDLE
	end
	local Handle = InstanceHandle.new(Value)
	HANDLES[Value] = Handle
	return Handle
end

local function StreamInstances(Stream: Stream): { any }
	local Instances = STREAM_INSTANCES[Stream]
	if Instances == nil then
		Instances = table.create(64)
		STREAM_INSTANCES[Stream] = Instances
	end
	return Instances
end
