use hex;
use jsonrpc;
use serde_json;

use std::collections::HashMap;

use bitcoin::blockdata::block::{Block, BlockHeader};
use bitcoin::blockdata::transaction::{Transaction, SigHashType};
use bitcoin::network::encodable::ConsensusDecodable;
use bitcoin::network::serialize::{RawDecoder};
use bitcoin::util::address::Address;
use bitcoin::util::hash::Sha256dHash;

use error::*;
use types::*;


/// Client implements a JSON-RPC client for the Bitcoin Core daemon or compatible APIs.
pub struct Client {
	client: jsonrpc::client::Client,
}

/// Arg is a simple enum to represent an argument value and its context.
enum Arg {
	Required(serde_json::Value),
	OptionalSet(serde_json::Value),
	OptionalDefault(serde_json::Value),
}

/// arg is used to quickly generate Arg instances.  For optional argument a default value can be
/// provided that will be used if the actual value was None.  If the default value doesn't matter
/// (f.e. for the last optional argument), it can be left empty, but a comma should still be 
/// present.
macro_rules! arg {
	($val:expr) => {
		Arg::Required(serde_json::to_value($val)?)
	};
	($val:expr, $def:expr) => {
		match $val {
			Some(v) => Arg::OptionalSet(serde_json::to_value(v)?),
			None => Arg::OptionalDefault(serde_json::to_value($def)?),
		}
	};
	($val:expr,) => { arg!($val, "") };
}

/// empty quickly creates an empty Vec<serde_json::Value>.
/// Used because using vec![] as default value lacks type annotation.
macro_rules! empty {
	() => { { let v: Vec<serde_json::Value> = vec![]; v } }
}

/// make_call does two things: 
/// 1. build the argument list by dropping unnecessary default values and
/// 2. make a request to the underlying jsonrpc client.
/// It returns the response object.
macro_rules! make_call {
	($self:ident, $method:expr) => { make_call!($self, $method,) };
	($self:ident, $method:expr, $($arg:expr),*) => {
		{
			// We want to truncate the argument to remove the trailing non-set optional arguments.
			// This makes sure we don't send default values if we don't really need to and this 
			// can prevent unexpected behaviour if the server changes its default values.
			let mut args = Vec::new();
			$( args.push($arg); )*
			while let Some(Arg::OptionalDefault(_)) = args.last() {
				args.pop();
			}
			let json_args = args.into_iter().map(|a| match a {
				Arg::Required(v) => v,
				Arg::OptionalSet(v) => v,
				Arg::OptionalDefault(v) => v,
			}).collect();
			let req = $self.client.build_request($method.to_string(), json_args);
			$self.client.send_request(&req).map_err(Error::from)
		}
	}
}

/// result_json converts a JSON response into the provided type.
macro_rules! result_json {
	($resp:ident, $json_type:ty) => {
		$resp.and_then(|r| r.into_result::<$json_type>().map_err(Error::from))
	}
}

/// result_raw converts a hex response into a Bitcoin data type.
/// This works both for Option types and regular types, however the implementation differs.
macro_rules! result_raw {
	($resp:ident, Option<$raw_type:ty>) => {
		{
			let hex_opt = $resp.and_then(|r| r.into_result::<Option<String>>()
					.map_err(Error::from))?;
			match hex_opt {
				Some(hex) => {
					let raw = hex::decode(hex)?;
					match <$raw_type>::consensus_decode(&mut RawDecoder::new(raw.as_slice())) {
						Ok(val) => Ok(Some(val)),
						Err(e) => Err(e.into()),
					}
				},
				None => Ok(None),
			}
		}
	};
	($resp:ident, $raw_type:ty) => {
		$resp.and_then(|r| r.into_result::<String>().map_err(Error::from))
			 .and_then(|h| hex::decode(h).map_err(Error::from))
			 .and_then(|r| <$raw_type>::consensus_decode(&mut RawDecoder::new(r.as_slice()))
					.map_err(Error::from))
	};
}

impl Client {
	/// Create a new Client.
	pub fn new(uri: String, user: Option<String>, pass: Option<String>) -> Client {
		Client {
			client: jsonrpc::client::Client::new(uri, user, pass),
		}
	}

	// Methods have identical casing to API methods on purpose.
	// Variants of API methods are formed using an underscore.

	pub fn getblock_raw(&mut self, hash: Sha256dHash) -> Result<Block, Error> {
		let resp = make_call!(self, "getblock", arg!(hash), arg!(0));
		result_raw!(resp, Block)
	}

	pub fn getblock_info(&mut self, hash: Sha256dHash) -> Result<GetBlockResult, Error> {
		let resp = make_call!(self, "getblock", arg!(hash), arg!(1));
		result_json!(resp, GetBlockResult)
	}
	//TODO(stevenroose) getblock_raw (should be serialized to
	// bitcoin::blockdata::Block) and getblock_txs

	pub fn getblockcount(&mut self) -> Result<usize, Error> {
		let resp = make_call!(self, "getblockcount");
		result_json!(resp, usize)
	}

	pub fn getblockhash(&mut self, height: u32) -> Result<Sha256dHash, Error> {
		let resp = make_call!(self, "getblockhash", arg!(height));
		result_json!(resp, Sha256dHash)
	}

	pub fn getblockheader(&mut self, hash: Sha256dHash) -> Result<BlockHeader, Error> {
		let resp = make_call!(self, "getblockheader", arg!(hash), arg!(true));
		result_raw!(resp, BlockHeader)
	}

	pub fn getblockheader_verbose(&mut self, hash: Sha256dHash) -> Result<GetBlockHeaderResult, Error> {
		let resp = make_call!(self, "getblockheader", arg!(hash), arg!(true));
		result_json!(resp, GetBlockHeaderResult)
	}

	pub fn getrawtransaction(
		&mut self,
		txid: Sha256dHash,
		block_hash: Option<Sha256dHash>,
	) -> Result<Option<Transaction>, Error> {
		let resp = make_call!(self, "getrawtransaction", arg!(txid), arg!(false), arg!(block_hash));
		result_raw!(resp, Option<Transaction>)
	}

	pub fn getrawtransaction_verbose(
		&mut self,
		txid: Sha256dHash,
		block_hash: Option<Sha256dHash>,
	) -> Result<Option<GetRawTransactionResult>, Error> {
		let resp = make_call!(self, "getrawtransaction", arg!(txid), arg!(true), arg!(block_hash));
		result_json!(resp, Option<GetRawTransactionResult>)
	}

	pub fn gettxout(
		&mut self,
		txid: Sha256dHash,
		vout: u32,
		include_mempool: Option<bool>,
	) -> Result<Option<GetTxOutResult>, Error> {
		let resp = make_call!(self, "gettxout", arg!(txid), arg!(vout), arg!(include_mempool,));
		result_json!(resp, Option<GetTxOutResult>)
	}

	pub fn listunspent(
		&mut self,
		minconf: Option<usize>,
		maxconf: Option<usize>,
		addresses: Option<Vec<Address>>,
		include_unsafe: Option<bool>,
		query_options: Option<HashMap<String, String>>,
	) -> Result<Vec<ListUnspentResult>, Error> {
		let resp = make_call!(self, "listunspent", arg!(minconf, 0), arg!(maxconf, 9999999),
			arg!(addresses, empty!()), arg!(include_unsafe, true), arg!(query_options,));
		result_json!(resp, Vec<ListUnspentResult>)
	}

	pub fn signrawtransaction(
		&mut self,
		tx: &[u8],
		utxos: Option<Vec<UTXO>>,
		private_keys: Option<Vec<Vec<u8>>>,
		sighash_type: Option<SigHashType>,
	) -> Result<SignRawTransactionResult, Error> {
		let sighash = sighash_string(sighash_type);
		let resp = make_call!(self, "signrawtransaction", arg!(hex::encode(tx)),
			arg!(utxos, empty!()), arg!(Some(empty!()), empty!()),//TODO(stevenroose) impl privkeys
			arg!(sighash,));
		result_json!(resp, SignRawTransactionResult)
	}
}

/// sighash_string converts a SigHashType object to a string representation used in the API.
fn sighash_string(sighash: Option<SigHashType>) -> Option<String> {
	match sighash {
		None => None,
		Some(sh) => Some(String::from(match sh {
			SigHashType::All => "ALL",
			SigHashType::None => "NONE",
			SigHashType::Single => "SINGLE",
			SigHashType::AllPlusAnyoneCanPay => "ALL|ANYONECANPAY",
			SigHashType::NonePlusAnyoneCanPay => "NONE|ANYONECANPAY",
			SigHashType::SinglePlusAnyoneCanPay => "SINGLE|ANYONECANPAY",
		})),
	}
}