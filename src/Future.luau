--!strict
--!optimize 2

--[=[
	Future: a call running on its own thread that you can wait on

	Wait gives you the results, or nothing if it timed out or was cancelled. Happened means it
	finished without an error, and Cancelled gives you the error if there was one

	Return type: New, IsFuture, FromWeakFuture, and Completed, Cancelled and Errored for Futures
	that have already ended
	Example usage:
		local Done = Future.New(function()
			return 1
		end)
		print(Done:Wait(5))
--]=]

local Types = require("./Types")

type Future<T...> = Types.Future<T...>
type Packed = Types.Packed

local Pack = table.pack :: (...any) -> Packed

local HASNT_HAPPENED = "Future hasn't happened yet"
local ALREADY_HAPPENED = "Future already happened or was cancelled"

local Future = {}
Future.__index = Future

type Private<T...> = {
	Result: Packed?,
	WasCancelled: boolean,

	Waiting: { thread },
	Timeouts: { thread },

	Thread: thread,
}

type FutureReference<T...> = setmetatable<{ Inst: Future<T...> }, { __mode: "v" }>
type TimeoutReference<T...> = setmetatable<{ Inst: Future<T...>, Caller: thread }, { __mode: "v" }>

local function TryRemove(Table: any, Value: any): boolean
	local Position = table.find(Table, Value)
	return Position ~= nil and table.remove(Table, Position) ~= nil
end

local function ToPrivate<T...>(self: Future<T...>): Private<T...>
	return self :: Private<T...>
end

local function FromWeakFuture<T...>(Object: any): Future<T...>?
	if type(Object) == "table" and Object.__future then
		local Private = ToPrivate(Object)
		Private.Waiting = Private.Waiting or {}
		Private.Timeouts = Private.Timeouts or {}

		return setmetatable(Object, Future)
	end

	return nil
end

local function Unpack<T...>(self: Private<T...> | Future<T...>): T...
	local Data = (self :: Private<T...>).Result
	if Data and Data[1] == true then
		return table.unpack(Data, 2, Data.n)
	end
	return nil :: any
end

local function Finish<T...>(self: Private<T...>, ...)
	for _, TimeoutThread in self.Timeouts do
		if coroutine.status(TimeoutThread) ~= "suspended" then
			continue
		end

		task.cancel(TimeoutThread)
	end

	for _, WaitingThread in self.Waiting do
		if coroutine.status(WaitingThread) ~= "suspended" then
			continue
		end

		coroutine.resume(WaitingThread, ...)
	end

	table.clear(self.Waiting)
	table.clear(self.Timeouts)
end

local function FutureIsHappening<T...>(self: Future<T...>): boolean
	local Private = ToPrivate(self)

	return not Private.WasCancelled and Private.Thread and coroutine.status(Private.Thread) == "suspended"
end

local function TryConsume<T...>(self: Private<T...>)
	local Argument = Unpack<<(any)>>(self)

	if Argument and type(Argument) == "table" and Argument.__future then
		self.Result = Argument.Result
		self.WasCancelled = Argument.WasCancelled
	end
end

local function FutureTask<Args..., Ret...>(Callback: (Args...) -> Ret..., Reference: FutureReference<Ret...>, ...: Args...)
	local Inst = Reference.Inst
	if not Inst then
		return
	end

	local Private = ToPrivate(Inst)

	Private.Result = Pack(pcall(Callback, ...))
	TryConsume(Private)

	Finish(Private, Unpack(Private))
end

local function TimeoutTask<T...>(Reference: TimeoutReference<T...>)
	local self = coroutine.running()

	local Inst = Reference.Inst
	if not Inst then
		return
	end

	if not FutureIsHappening(Inst) then
		return
	end

	local Caller = Reference.Caller
	if not Caller then
		return
	end

	if coroutine.status(Caller) ~= "suspended" then
		return
	end

	local Private = ToPrivate(Inst)
	if not TryRemove(Private.Waiting, Caller) then
		return
	end

	if not TryRemove(Private.Timeouts, self) then
		return
	end

	if #Private.Timeouts == 0 and #Private.Waiting == 0 then
		Inst:Cancel()
	end

	coroutine.resume(Caller)
end

local function New<Args..., Ret...>(Callback: (Args...) -> Ret..., ...: Args...): Future<Ret...>
	local Body = { WasCancelled = false, Waiting = {}, Timeouts = {} } :: Private<Ret...>
	local NewFuture: Future<Ret...> = setmetatable(Body :: any, Future) :: any

	local WeakReference: FutureReference<Ret...> = setmetatable({
		Inst = NewFuture,
		Caller = nil,
	}, { __mode = "v" })
	Body.Thread = task.spawn(FutureTask, Callback, WeakReference, ...)

	return NewFuture
end

function Future.Wait<T...>(self: Future<T...>, Timeout: number?): T...
	if not FutureIsHappening(self) then
		return Unpack(self)
	end

	local Private, Caller = ToPrivate(self), coroutine.running()
	table.insert(Private.Waiting, Caller)

	if Timeout then
		local WeakReference: TimeoutReference<T...> = setmetatable({
			Inst = self,
			Caller = Caller,
		}, { __mode = "v" })

		table.insert(Private.Timeouts, task.delay(Timeout, TimeoutTask, WeakReference))
	end

	return coroutine.yield()
end

function Future.Cancel<T...>(self: Future<T...>): ()
	assert(FutureIsHappening(self), ALREADY_HAPPENED)
	local Private = ToPrivate(self)
	Private.WasCancelled = true

	Finish(Private)

	local Thread = Private.Thread
	if Thread and coroutine.status(Thread) == "suspended" then
		task.cancel(Thread)
	end
end

function Future.Happened<T...>(self: Future<T...>): boolean
	if FutureIsHappening(self) then
		return false
	end

	local Private = ToPrivate(self)
	if Private.WasCancelled then
		return false
	end

	local Data = Private.Result
	if Data then
		local Success = Data[1]
		if type(Success) == "boolean" then
			return Success
		end
	end

	return true
end

function Future.Cancelled<T...>(self: Future<T...>): (boolean, any)
	local Private = ToPrivate(self)

	local Data = Private.Result
	if Data and Data[1] == false then
		return true, Data[2]
	end

	if Private.WasCancelled then
		return true
	end

	return false
end

function Future.Data<T...>(self: Future<T...>): T...
	assert(not FutureIsHappening(self), HASNT_HAPPENED)
	return Unpack(self)
end

local function IsFuture(Value: any): boolean
	return if Value ~= nil then getmetatable(Value) == Future else false
end

local function NewCompleted<T...>(...: T...): Future<T...>
	local Body = {
		__future = true,

		Result = Pack(true, ...),
		WasCancelled = false,
		Waiting = {},
		Timeouts = {},
	} :: Private<T...>

	return setmetatable(Body :: any, Future) :: any
end

local function NewCancelled<T...>(): Future<T...>
	local Body = {
		__future = true,

		WasCancelled = true,
		Waiting = {},
		Timeouts = {},
	} :: Private<T...>

	return setmetatable(Body :: any, Future) :: any
end

local function NewErrored<T...>(ErrorMessage: string): Future<T...>
	local Body = {
		__future = true,

		WasCancelled = true,
		Waiting = {},
		Timeouts = {},
		Result = table.pack(false, ErrorMessage),
	} :: Private<T...>

	return setmetatable(Body :: any, Future) :: any
end

table.freeze(Future)
return table.freeze({
	New = New,
	IsFuture = IsFuture,
	FromWeakFuture = FromWeakFuture,

	Completed = NewCompleted,
	Cancelled = NewCancelled,
	Errored = NewErrored,
})
