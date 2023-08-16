import { Property, Versioned } from '@std/types';
import { Text } from '@std/data';

/**
 * Primary E-Mail Address
 *
 * Primary E-Mail Address of an entity.
 *
 * @id primary-email
 * @version 1
 */
interface V1 extends Property {
    version: 1;

    oneOf: Text;
}

export type PrimaryEMail = Versioned<V1>;