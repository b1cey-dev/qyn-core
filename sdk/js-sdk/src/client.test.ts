import { QuynClient } from './client';

describe('QuynClient', () => {
  it('constructs with default URL', () => {
    const c = new QuynClient();
    expect(c).toBeDefined();
  });
  it('constructs with custom URL', () => {
    const c = new QuynClient('http://localhost:8545');
    expect(c).toBeDefined();
  });
});
