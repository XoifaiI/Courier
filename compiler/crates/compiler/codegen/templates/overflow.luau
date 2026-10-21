local function Overflow(Stream: Stream, Writer: any, ...: any)
@BROADCAST@
	if debug.info(3, "f") == Overflow then
		error(`Courier: a message cannot fit a stream of {buffer.len(Stream) - HEADER - 4} bytes even when empty, so it is dropped`, 5)
	end
	local Handler = OnFull
	if Handler == nil then
		error(`Courier: a stream of {buffer.len(Stream) - HEADER - 4} bytes is full and no full handler is set, so the message is dropped; call SetOnFull or make the stream larger`, 3)
	end
	local Empty = IsEmpty(Stream)
	Handler(Stream)
	if Empty then
		error(`Courier: a message cannot fit a stream of {buffer.len(Stream) - HEADER - 4} bytes even when empty, so it is dropped`, 3)
	end
	Writer(Stream, ...)
end
