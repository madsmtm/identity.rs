import { setup } from '../../support/setup';
import { revokeVC } from "../../../examples/dist/web/0_basic/7_revoke_vc";

describe(
  "revokeVC",
  () => {
    it("Revoke Credential", async () => {
      await setup(revokeVC)
    });
  }
);
