// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::Result;

fn main() -> Result<()> {
    let mut client = wicket_dbg::Client::connect("::1:9010")?;
    client.send(&wicket_dbg::Cmd::Load("SOME RECORDING".into()))?;
    Ok(())
}
